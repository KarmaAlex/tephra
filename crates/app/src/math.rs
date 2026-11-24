//! Math typesetting: RaTeX (native KaTeX port) rasterised at device scale, cached.
//! The few formulas RaTeX rejects fall back to MathJax running in an embedded QuickJS,
//! loaded only the first time it is needed (4 of 10,914 formulas in the corpus).

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use gpui_kit::{Hsla, RenderImage, Rgba};

/// A rendered formula, sized in logical pixels. The image is already decoded (BGRA), so GPUI
/// paints it in the frame it first appears in: an `Arc<Image>` is decoded asynchronously and
/// painted nothing for a frame, the flicker while typing in a math block.
#[derive(Clone)]
pub struct MathImage {
    pub image: Arc<RenderImage>,
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

/// Typesets `tex`, waiting for the MathJax fallback however long it takes. `None` means both
/// engines failed (the caller shows the source). The editor uses [`render_within`].
#[cfg(test)]
pub fn render(tex: &str, display: bool, font_px: f32, color: Hsla, scale: f32) -> Option<MathImage> {
    match render_within(tex, display, font_px, color, scale, None) {
        Typeset::Done(out) => out,
        Typeset::Pending => None, // not reached: without a budget the fallback is waited for
    }
}

/// The outcome of [`render_within`].
pub enum Typeset {
    /// Typeset (or both engines failed: None).
    Done(Option<MathImage>),
    /// The MathJax fallback is still running on its thread; ask again on a later frame.
    Pending,
}

/// MathJax fallbacks still running after their budget ran out.
fn pending() -> &'static Mutex<std::collections::HashSet<Key>> {
    static P: OnceLock<Mutex<std::collections::HashSet<Key>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

fn store(key: Key, out: Option<MathImage>) {
    if let Ok(mut c) = cache().lock() {
        // decoded images are raw RGBA, much bigger than the PNG bytes this cache used to hold:
        // bound it so a whole-vault browsing session can't grow it without limit
        if c.len() >= 1024 {
            c.clear();
        }
        c.insert(key, out);
    }
}

/// Like [`render`], but waits at most `budget` for the MathJax fallback (RaTeX is always
/// synchronous): within a frame, a usual fallback (1–3 ms) still shows at once, while a slow one
/// (MathJax starting, or a whole paragraph read as a formula while `\$` is being typed)
/// finishes on a thread and comes back as `Pending` until it is in the cache.
pub fn render_within(tex: &str, display: bool, font_px: f32, color: Hsla, scale: f32, budget: Option<std::time::Duration>) -> Typeset {
    let rgba: Rgba = color.into();
    let packed = ((rgba.r * 255.0) as u32) << 24 | ((rgba.g * 255.0) as u32) << 16 | ((rgba.b * 255.0) as u32) << 8 | (rgba.a * 255.0) as u32;
    let key = Key { tex: tex.to_string(), display, font_px_x10: (font_px * 10.0) as u32, rgba: packed, scale_x100: (scale * 100.0) as u32 };
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Typeset::Done(hit);
    }
    if pending().lock().is_ok_and(|p| p.contains(&key)) {
        return Typeset::Pending;
    }
    if let Some(img) = ratex(tex, display, font_px, rgba, scale) {
        store(key, Some(img.clone()));
        return Typeset::Done(Some(img));
    }
    let Some(reply) = mathjax_request(tex, display) else {
        store(key, None);
        return Typeset::Done(None);
    };
    let got = match budget {
        None => reply.recv().map_err(|_| ()),
        Some(d) => match reply.recv_timeout(d) {
            Ok(v) => Ok(v),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // finish on a thread; the cache has it once done
                if let Ok(mut p) = pending().lock() {
                    p.insert(key.clone());
                }
                std::thread::spawn(move || {
                    let svg = reply.recv().ok().flatten();
                    let out = svg.and_then(|svg| rasterise_mathjax(&svg, key.font_px_x10 as f32 / 10.0, rgba, key.scale_x100 as f32 / 100.0));
                    store(key.clone(), out);
                    if let Ok(mut p) = pending().lock() {
                        p.remove(&key);
                    }
                });
                return Typeset::Pending;
            }
        },
    };
    let out = got.ok().flatten().and_then(|svg| rasterise_mathjax(&svg, font_px, rgba, scale));
    store(key, out.clone());
    Typeset::Done(out)
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
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(MathImage { image: render_image(w, h, img.into_raw())?, width, height, baseline })
}

/// Straight-alpha RGBA pixels -> a decoded GPUI image (BGRA, like GPUI's own PNG decoding).
fn render_image(w: u32, h: u32, mut rgba: Vec<u8>) -> Option<Arc<RenderImage>> {
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let frame = image::Frame::new(image::RgbaImage::from_raw(w, h, rgba)?);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

// ---- MathJax fallback (QuickJS worker thread, created on first use) -----------------

type Job = (String, bool, mpsc::Sender<Option<String>>);

static MATHJAX_BUNDLE: &str = include_str!("../assets/mathjax-svg.js");

/// MathJax's SVG for a formula (its QuickJS worker; also used by PDF export).
pub(crate) fn mathjax_svg(tex: &str, display: bool) -> Option<String> {
    mathjax_request(tex, display)?.recv().ok().flatten()
}

/// Queues a formula on the MathJax worker; the SVG (None: MathJax failed) arrives on the receiver.
fn mathjax_request(tex: &str, display: bool) -> Option<mpsc::Receiver<Option<String>>> {
    let (rtx, rrx) = mpsc::channel();
    worker().lock().ok()?.send((tex.to_string(), display, rtx)).ok()?;
    Some(rrx)
}

/// Starts the MathJax worker and loads its bundle (~90 ms) in the background, without waiting.
/// For previews of formulas being typed: every unfinished formula (`\frac{a`) goes to the
/// fallback, and the first one would otherwise wait for the load inside a frame.
pub(crate) fn warm_mathjax() {
    static WARMED: std::sync::Once = std::sync::Once::new();
    WARMED.call_once(|| {
        let (rtx, _) = mpsc::channel();
        if let Ok(tx) = worker().lock() {
            // a real formula, error included: the first typeset also sets up MathJax's own state
            let _ = tx.send(("\\frac{a}{b}^{2}+\\".to_string(), false, rtx));
        }
    });
}

fn worker() -> &'static Mutex<mpsc::Sender<Job>> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    WORKER.get_or_init(|| {
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
    })
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

/// MathJax's SVG rasterised at device scale, like RaTeX's output.
fn rasterise_mathjax(svg: &str, font_px: f32, rgba: Rgba, scale: f32) -> Option<MathImage> {
    let ex = font_px * 0.442;
    let w = (svg_attr(&svg, "width")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
    let h = (svg_attr(&svg, "height")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
    let va = svg_attr(&svg, "style")
        .and_then(|s| s.split("vertical-align:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(';').strip_suffix("ex"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    let hex = format!("#{:02x}{:02x}{:02x}", (rgba.r * 255.0) as u8, (rgba.g * 255.0) as u8, (rgba.b * 255.0) as u8);
    let svg = replace_attr(&replace_attr(svg, "width", &format!("{w:.1}")), "height", &format!("{h:.1}")).replace("currentColor", &hex);
    // rasterised here at device scale, like RaTeX's output (MathJax's SVG is all paths)
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let s = scale.max(1.0);
    let mut pix = resvg::tiny_skia::Pixmap::new((w * s).ceil() as u32, (h * s).ceil() as u32)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pix.as_mut());
    let rgba = pix.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    });
    let image = render_image(pix.width(), pix.height(), rgba.collect())?;
    Some(MathImage { image, width: w, height: h, baseline: h + va * ex })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::hsla;

    /// A fallback slower than the frame's budget finishes on a thread: `Pending` until the
    /// cache has it, then the image (never stuck pending, never typeset twice at once).
    #[test]
    fn slow_fallback_finishes_on_a_thread() {
        let (tex, c) = ("y+\\", hsla(0., 0., 0., 1.)); // RaTeX rejects it, MathJax draws it
        assert!(ratex(tex, false, 17.0, c.into(), 1.0).is_none());
        let now = || render_within(tex, false, 17.0, c, 1.0, Some(std::time::Duration::ZERO));
        assert!(matches!(now(), Typeset::Pending), "a zero budget can't wait for MathJax");
        assert!(matches!(now(), Typeset::Pending), "asked again while running: still pending");
        let t = std::time::Instant::now();
        loop {
            match now() {
                Typeset::Done(img) => break assert!(img.is_some(), "MathJax result cached"),
                Typeset::Pending => assert!(t.elapsed().as_secs() < 20, "fallback never finished"),
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

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
            let m = render(tex, d, 16.0, hsla(0., 0., 0., 1.), 1.0);
            let m = m.unwrap_or_else(|| panic!("{tex}"));
            // a real raster with ink in it (the MathJax fallback is rasterised here too)
            let frame = m.image.as_bytes(0).expect("one frame");
            assert!(frame.chunks_exact(4).any(|p| p[3] > 0), "{tex}: empty image");
        }
    }
}
