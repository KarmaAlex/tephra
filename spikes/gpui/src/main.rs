//! Perf spike: GPUI (via gpui-kit) window showing one vault note in reading view,
//! with math typeset by MathJax running in an embedded QuickJS engine (GPUI has no
//! math layout of its own). Approach adapted from gpui-kit's Apache-2.0 markdown example.
//!
//! env: NOTE_PATH (note to show), MATHJAX_BUNDLE (assets/mathjax-svg.js)
//! `spike-gpui --mathbench <formulas.json>` times QuickJS+MathJax per formula and exits.
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

static MATH_OK: AtomicUsize = AtomicUsize::new(0);
static MATH_FAIL: AtomicUsize = AtomicUsize::new(0);
use std::time::Instant;

use gpui_kit::component::text::{
    markdown_ast, InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin, TextView, TextViewState,
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

// ---------- QuickJS + MathJax worker ----------
type MathJob = (String, bool, mpsc::Sender<Option<String>>);

fn tex2svg(source: &str, display: bool) -> Option<String> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<MathJob>>> = OnceLock::new();
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<MathJob>();
        std::thread::spawn(move || {
            let rt = rquickjs::Runtime::new().expect("quickjs runtime");
            let ctx = rquickjs::Context::full(&rt).expect("quickjs context");
            let path = std::env::var("MATHJAX_BUNDLE").unwrap_or_else(|_| "assets/mathjax-svg.js".into());
            let bundle = std::fs::read_to_string(&path).expect("read MathJax bundle");
            ctx.with(|ctx| {
                use rquickjs::CatchResultExt as _;
                if let Err(e) = ctx.eval::<(), _>(bundle).catch(&ctx) {
                    panic!("eval MathJax bundle: {e}");
                }
            });
            for (src, display, reply) in rx {
                let svg = ctx.with(|ctx| {
                    let f: rquickjs::Function = ctx.globals().get("tex2svg").ok()?;
                    f.call::<_, String>((src, display)).ok()
                });
                let _ = reply.send(svg.filter(|s| !s.is_empty()));
            }
        });
        Mutex::new(tx)
    });
    let (rtx, rrx) = mpsc::channel();
    tx.lock().ok()?.send((source.to_string(), display, rtx)).ok()?;
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

#[derive(Clone)]
struct MathImage { image: Arc<Image>, w: f32, h: f32, baseline: f32 }

fn math_image(source: &str, display: bool, font_px: f32, fg: Hsla) -> Option<MathImage> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<MathImage>>>> = OnceLock::new();
    let rgba: Rgba = fg.into();
    let hex = format!("#{:02x}{:02x}{:02x}", (rgba.r * 255.0) as u8, (rgba.g * 255.0) as u8, (rgba.b * 255.0) as u8);
    let key = format!("{display}\0{font_px:.1}\0{hex}\0{source}");
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok()?.get(&key) {
        return hit.clone();
    }
    let out = tex2svg(source, display).and_then(|svg| {
        let ex = font_px * 0.5;
        let w = (svg_attr(&svg, "width")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
        let h = (svg_attr(&svg, "height")?.strip_suffix("ex")?.parse::<f32>().ok()? * ex).ceil().max(1.0);
        let vb: Vec<f32> = svg_attr(&svg, "viewBox")?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        let baseline = if vb.len() == 4 && vb[3] > 0.0 { (-vb[1] / vb[3] * h).clamp(0.0, h) } else { h };
        let svg = replace_attr(&replace_attr(&svg, "width", &format!("{w:.1}")), "height", &format!("{h:.1}"))
            .replace("currentColor", &hex);
        Some(MathImage { image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())), w, h, baseline })
    });
    if out.is_some() { MATH_OK.fetch_add(1, Ordering::Relaxed); } else { MATH_FAIL.fetch_add(1, Ordering::Relaxed); }
    cache.lock().ok()?.insert(key, out.clone());
    out
}

#[derive(Clone, Debug)]
struct MathNode { source: String }

#[derive(Clone)]
struct BlockMath;
impl MarkdownPlugin for BlockMath {
    fn is_block(&self) -> bool { true }
    fn name(&self) -> &str { "math" }
    fn parse(&self, node: &markdown_ast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Math(m) = node else { return None };
        Some(MarkdownNode::new("math", MathNode { source: m.value.clone() }).text(m.value.clone()))
    }
    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let src = node.data::<MathNode>().map(|m| m.source.clone()).unwrap_or_default();
        let font_px = f32::from(window.text_style().font_size.to_pixels(window.rem_size())) * 1.18;
        let el = match math_image(&src, true, font_px, cx.theme().foreground) {
            Some(m) => img(m.image).object_fit(ObjectFit::Contain).w(px(m.w)).h(px(m.h)).into_any_element(),
            None => div().child(src).into_any_element(),
        };
        div().w_full().flex().justify_center().py_1().child(el)
    }
}

#[derive(Clone)]
struct InlineMath;
impl MarkdownPlugin for InlineMath {
    fn name(&self) -> &str { "inline-math" }
    fn parse(&self, node: &markdown_ast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::InlineMath(m) = node else { return None };
        Some(MarkdownNode::new("inline-math", MathNode { source: m.value.clone() }).text(m.value.clone()))
    }
    fn render_inline(&self, node: &MarkdownNode, context: &InlineRenderContext, _w: &mut Window, _cx: &mut App) -> Option<InlineElement> {
        let src = node.data::<MathNode>()?.source.clone();
        let m = math_image(&src, false, f32::from(context.font_size()), context.text_style().color)?;
        Some(InlineElement::new(img(m.image).object_fit(ObjectFit::Contain).w(px(m.w)).h(px(m.h))).with_baseline(px(m.baseline)))
    }
}

// ---------- window ----------
struct Spike { state: Entity<TextViewState> }
impl Render for Spike {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background).p_4().child(
            TextView::new(&self.state).plugin(InlineMath).plugin(BlockMath).scrollable(true),
        )
    }
}

fn mathbench(path: &str) {
    let raw = std::fs::read_to_string(path).expect("read formulas");
    // formulas.json: one {"tex":..,"display":..} per line (written by tools/corpus).
    let items: Vec<(String, bool)> = raw.lines().filter(|l| !l.trim().is_empty()).map(|l| {
        let display = l.contains("\"display\":true");
        let t = l.split("\"tex\":").nth(1).unwrap_or("\"\"");
        (unescape_json_string(t), display)
    }).collect();
    let t0 = Instant::now();
    let _ = tex2svg("x", false); // load bundle
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut times: Vec<f64> = Vec::new();
    let mut fails = 0;
    for (tex, d) in &items {
        let t = Instant::now();
        if tex2svg(tex, *d).is_none() { fails += 1; }
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| times[((times.len() as f64 * p) as usize).min(times.len() - 1)];
    println!("MATHBENCH {{\"formulas\":{},\"failures\":{},\"bundle_load_ms\":{:.1},\"total_ms\":{:.1},\"p50_ms\":{:.2},\"p95_ms\":{:.2},\"max_ms\":{:.2}}}",
        items.len(), fails, load_ms, times.iter().sum::<f64>(), pct(0.5), pct(0.95), pct(1.0));
}

fn unescape_json_string(s: &str) -> String {
    // minimal JSON string decoder for the first string literal in `s`
    let mut out = String::new();
    let mut it = s.trim_start().chars().skip(1);
    while let Some(c) = it.next() {
        match c {
            '"' => break,
            '\\' => match it.next() {
                Some('n') => out.push('\n'), Some('t') => out.push('\t'), Some('r') => out.push('\r'),
                Some('u') => { let h: String = it.by_ref().take(4).collect(); if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) { out.push(ch) } }
                Some(o) => out.push(o), None => break,
            },
            c => out.push(c),
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--mathbench") {
        mathbench(&args[2]);
        return;
    }
    let t0 = Instant::now();
    let note = std::fs::read_to_string(std::env::var("NOTE_PATH").expect("NOTE_PATH")).expect("read note");
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let opts = WindowOptions { window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1200.), px(900.)), cx))), ..Default::default() };
        gpui_kit::open_window(opts, cx, move |window, cx| {
            window.on_next_frame(move |w, _| {
                println!("READY {:.1}", t0.elapsed().as_secs_f64() * 1000.0);
                w.on_next_frame(|_, _| println!("MATH rendered={} failed={}", MATH_OK.load(Ordering::Relaxed), MATH_FAIL.load(Ordering::Relaxed)));
                w.refresh();
            });
            cx.new(|cx| Spike { state: cx.new(|cx| TextViewState::markdown(note.as_str(), cx)) })
        })
        .expect("open window");
    });
}
