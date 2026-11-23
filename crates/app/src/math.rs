//! Math typesetting: RaTeX (native KaTeX port) rasterised at device scale, cached.
//! The few formulas RaTeX rejects fall back to MathJax running in an embedded QuickJS,
//! loaded only the first time it is needed (4 of 10,914 formulas in the corpus).

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use gpui_kit::{Hsla, Image, ImageFormat, Rgba};

/// A rendered formula, sized in logical pixels.
#[derive(Clone)]
pub struct MathImage {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
    /// Distance from the top to the baseline.
    pub baseline: f32,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    tex: String,
    display: bool,
    font_px_x10: u32,
    rgba: u32,
    scale_x100: u32,
}

fn cache() -> &'static Mutex<HashMap<Key, Option<MathImage>>> {
    static C: OnceLock<Mutex<HashMap<Key, Option<MathImage>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Obsidian/MathJax compatibility tweaks applied before RaTeX.
fn normalise(tex: &str) -> String {
    tex.replace("\\textemdash", "\\text{—}")
        // MathJax as configured by Obsidian does not number `align` rows.
        .replace("\\begin{align}", "\\begin{aligned}")
        .replace("\\end{align}", "\\end{aligned}")
        .replace("\\begin{align*}", "\\begin{aligned}")
        .replace("\\end{align*}", "\\end{aligned}")
}

/// Typesets `tex`. `None` means both engines failed (the caller shows the source).
pub fn render(tex: &str, display: bool, font_px: f32, color: Hsla, scale: f32) -> Option<MathImage> {
    let rgba: Rgba = color.into();
    let packed = ((rgba.r * 255.0) as u32) << 24 | ((rgba.g * 255.0) as u32) << 16 | ((rgba.b * 255.0) as u32) << 8 | (rgba.a * 255.0) as u32;
    let key = Key { tex: tex.to_string(), display, font_px_x10: (font_px * 10.0) as u32, rgba: packed, scale_x100: (scale * 100.0) as u32 };
    if let Some(hit) = cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let out = ratex(tex, display, font_px, rgba, scale).or_else(|| mathjax(tex, display, font_px, rgba));
    if let Ok(mut c) = cache().lock() {
        c.insert(key, out.clone());
    }
    out
}

fn ratex(tex: &str, display: bool, font_px: f32, rgba: Rgba, scale: f32) -> Option<MathImage> {
    use ratex_layout::{layout, layout_options::LayoutOptions, to_display::to_display_list};
    use ratex_types::{Color, MathStyle};
    let src = normalise(tex);
    let nodes = std::panic::catch_unwind(|| ratex_parser::parse(&src)).ok()?.ok()?;
    let opts = LayoutOptions {
        style: if display { MathStyle::Display } else { MathStyle::Text },
        color: Color::new(rgba.r, rgba.g, rgba.b, rgba.a),
        ..Default::default()
    };
    let list = std::panic::catch_unwind(|| to_display_list(&layout(&nodes, &opts))).ok()?;
    let pad = 1.0;
    let ropts = ratex_render::RenderOptions {
        font_size: font_px,
        padding: pad,
        background_color: Color::new(0.0, 0.0, 0.0, 0.0),
        device_pixel_ratio: scale.max(1.0),
        ..Default::default()
    };
    let png = ratex_render::render_to_png(&list, &ropts).ok()?;
    let width = (list.width as f32 * font_px + 2.0 * pad).ceil();
    let height = ((list.height + list.depth) as f32 * font_px + 2.0 * pad).ceil();
    let baseline = pad + list.height as f32 * font_px;
    Some(MathImage { image: Arc::new(Image::from_bytes(ImageFormat::Png, png)), width, height, baseline })
}

// ---- MathJax fallback (QuickJS worker thread, created on first use) -----------------

type Job = (String, bool, mpsc::Sender<Option<String>>);

static MATHJAX_BUNDLE: &str = include_str!("../assets/mathjax-svg.js");

/// MathJax's SVG for a formula (its QuickJS worker; also used by PDF export).
pub(crate) fn mathjax_svg(tex: &str, display: bool) -> Option<String> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("mathjax".into())
            .spawn(move || {
                let Ok(rt) = rquickjs::Runtime::new() else { return };
                let Ok(ctx) = rquickjs::Context::full(&rt) else { return };
                let loaded = ctx.with(|ctx| ctx.eval::<(), _>(MATHJAX_BUNDLE).is_ok());
                for (src, display, reply) in rx {
                    let svg = loaded
                        .then(|| {
                            ctx.with(|ctx| {
                                let f: rquickjs::Function = ctx.globals().get("tex2svg").ok()?;
                                f.call::<_, String>((src, display)).ok()
                            })
                        })
                        .flatten();
                    let _ = reply.send(svg.filter(|s| !s.is_empty()));
                }
            })
            .expect("spawn mathjax thread");
        Mutex::new(tx)
    });
    let (rtx, rrx) = mpsc::channel();
    tx.lock().ok()?.send((tex.to_string(), display, rtx)).ok()?;
    rrx.recv().ok().flatten()
}

fn svg_attr<'a>(svg: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!(" {name}=\"");
    let start = svg.find(&pat)? + pat.len();
    let end = svg[start..].find('"')?;
    Some(&svg[start..start + end])
}

fn replace_attr(svg: &str, name: &str, value: &str) -> String {
    let pat = format!(" {name}=\"");
    let Some(start) = svg.find(&pat).map(|s| s + pat.len()) else { return svg.to_string() };
    let Some(end) = svg[start..].find('"') else { return svg.to_string() };
    format!("{}{}{}", &svg[..start], value, &svg[start + end..])
}

fn mathjax(tex: &str, display: bool, font_px: f32, rgba: Rgba) -> Option<MathImage> {
    let svg = mathjax_svg(tex, display)?;
    let ex = font_px * 0.442;
    let w = (svg_attr(&svg, "width")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
    let h = (svg_attr(&svg, "height")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
    let va = svg_attr(&svg, "style")
        .and_then(|s| s.split("vertical-align:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(';').strip_suffix("ex"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    let hex = format!("#{:02x}{:02x}{:02x}", (rgba.r * 255.0) as u8, (rgba.g * 255.0) as u8, (rgba.b * 255.0) as u8);
    let svg = replace_attr(&replace_attr(&svg, "width", &format!("{w:.1}")), "height", &format!("{h:.1}")).replace("currentColor", &hex);
    Some(MathImage { image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())), width: w, height: h, baseline: h + va * ex })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::hsla;

    #[test]
    fn ratex_renders_and_caches() {
        let a = render("\\frac{a}{b}", false, 16.0, hsla(0., 0., 0., 1.), 1.0).expect("render");
        assert!(a.width > 5.0 && a.height > 10.0 && a.baseline > 0.0 && a.baseline < a.height);
        let b = render("\\frac{a}{b}", false, 16.0, hsla(0., 0., 0., 1.), 1.0).unwrap();
        assert!(Arc::ptr_eq(&a.image, &b.image));
    }

    #[test]
    fn corpus_failures_are_covered() {
        // the constructs RaTeX rejects in the corpus (synthetic, not from notes)
        for (tex, d) in [("x \\textemdash y", false), ("2^\\sqrt{t}", false), ("\\text{N\\\\A}", true)] {
            assert!(render(tex, d, 16.0, hsla(0., 0., 0., 1.), 1.0).is_some(), "{tex}");
        }
    }
}
