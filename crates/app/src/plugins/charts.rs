//! Charts (replaces the community plugin "Obsidian Charts"): ```` ```chart ```` blocks with its
//! YAML syntax, for the kinds your vault uses: `bar` and `line` (several series, `labels`,
//! `beginAtZero`, `fill`, `width`, `labelColors`, `bestFit`). One layout feeds both the editor
//! (drawn on a canvas) and PDF export (Typst drawing commands), so they look the same.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub title: String,
    pub data: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: String,
    pub labels: Vec<String>,
    pub series: Vec<Series>,
    pub begin_at_zero: bool,
    pub fill: bool,
    /// Share of the available width, 0..1.
    pub width: f32,
    pub label_colors: bool,
    pub best_fit: bool,
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(s).to_string()
}

/// `[a, "b, c", 3]` → items.
fn inline_list(s: &str) -> Vec<String> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let mut out = Vec::new();
    let (mut cur, mut quote) = (String::new(), None);
    for ch in s.chars() {
        match (ch, quote) {
            ('"' | '\'', None) => quote = Some(ch),
            (c, Some(q)) if c == q => quote = None,
            (',', None) => out.push(unquote(&std::mem::take(&mut cur))),
            (c, _) => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(unquote(&cur));
    }
    out
}

/// The subset of YAML the Charts plugin's blocks use: `key: value`, inline `[…]` lists, block
/// `- item` lists, and `series:` as a list of `title`/`data` maps.
pub fn parse(src: &str) -> Result<Spec, String> {
    // a block indented as a whole (6 of your 10 chart blocks start with a tab) reads like an
    // unindented one: the first line's indentation comes off every line
    let first = src.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let pre = &first[..first.len() - first.trim_start().len()];
    let dedented: String = src.lines().map(|l| l.strip_prefix(pre).unwrap_or(l)).collect::<Vec<_>>().join("\n");
    let src = dedented.as_str();
    let mut kind = String::new();
    let mut labels = Vec::new();
    let mut series: Vec<Series> = Vec::new();
    let mut flags: std::collections::HashMap<String, String> = Default::default();
    let mut section: Option<&str> = None;
    for raw in src.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indented = raw.starts_with([' ', '\t']);
        let line = raw.trim();
        if !indented {
            let (k, v) = line.split_once(':').ok_or_else(|| format!("not a \"key: value\" line: {line}"))?;
            let v = v.trim();
            section = None;
            match k.trim() {
                "type" => kind = unquote(v),
                "labels" if v.is_empty() => section = Some("labels"),
                "labels" => labels = inline_list(v),
                "series" => section = Some("series"),
                other => {
                    flags.insert(other.to_string(), unquote(v));
                }
            }
            continue;
        }
        match section {
            Some("labels") => labels.push(unquote(line.trim_start_matches('-').trim())),
            Some("series") => {
                let item = line.strip_prefix('-').map(str::trim);
                if item.is_some() || series.is_empty() {
                    series.push(Series { title: String::new(), data: Vec::new() });
                }
                let kv = item.unwrap_or(line);
                if let Some((k, v)) = kv.split_once(':') {
                    let s = series.last_mut().expect("pushed");
                    match k.trim() {
                        "title" => s.title = unquote(v),
                        "data" => s.data = inline_list(v).iter().map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if kind.is_empty() {
        return Err("no chart type".into());
    }
    let flag = |k: &str, d: bool| flags.get(k).map(|v| v == "true").unwrap_or(d);
    let width = flags.get("width").and_then(|w| w.trim_end_matches('%').parse::<f32>().ok()).map(|w| (w / 100.0).clamp(0.1, 1.0)).unwrap_or(1.0);
    Ok(Spec { kind, labels, series, begin_at_zero: flag("beginAtZero", false), fill: flag("fill", false), width, label_colors: flag("labelColors", false), best_fit: flag("bestFit", false) })
}

/// Chart.js's default colours (what the plugin shows): (r, g, b).
const PALETTE: [(u8, u8, u8); 6] = [(255, 99, 132), (54, 162, 235), (255, 206, 86), (75, 192, 192), (153, 102, 255), (255, 159, 64)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub f32);

/// A drawing primitive in chart coordinates (origin top left, same unit as the size given).
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Rgba, stroke: Option<Rgba> },
    Line { pts: Vec<(f32, f32)>, color: Rgba, width: f32 },
    Polygon { pts: Vec<(f32, f32)>, fill: Rgba },
    Text { x: f32, y: f32, text: String, size: f32, color: Rgba, align: Align },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Round tick step for a range (1, 2, 5 × 10ⁿ).
fn nice_step(range: f64, ticks: usize) -> f64 {
    let raw = (range / ticks as f64).max(1e-9);
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    let n = if f <= 1.0 { 1.0 } else if f <= 2.0 { 2.0 } else if f <= 5.0 { 5.0 } else { 10.0 };
    n * mag
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{:.2}", v).trim_end_matches('0').trim_end_matches('.').to_string() }
}

/// Lays the chart out in a `w`×`h` box. `fg`/`grid` are the text and grid colours.
pub fn layout(s: &Spec, w: f32, h: f32, fg: Rgba, grid: Rgba) -> Vec<Prim> {
    let mut out = Vec::new();
    let font = (h / 22.0).clamp(7.0, 12.0);
    // legend
    let legend_h = if s.series.iter().any(|x| !x.title.is_empty()) { font * 2.0 } else { 0.0 };
    let mut lx = w / 2.0 - s.series.iter().map(|x| x.title.len() as f32 * font * 0.55 + font * 2.5).sum::<f32>() / 2.0;
    for (i, ser) in s.series.iter().enumerate() {
        let (r, g, b) = PALETTE[i % PALETTE.len()];
        out.push(Prim::Rect { x: lx, y: font * 0.4, w: font * 1.6, h: font, fill: Rgba(r, g, b, 0.25), stroke: Some(Rgba(r, g, b, 1.0)) });
        out.push(Prim::Text { x: lx + font * 2.0, y: font * 0.2, text: ser.title.clone(), size: font, color: fg, align: Align::Left });
        lx += ser.title.len() as f32 * font * 0.55 + font * 2.5;
    }
    // value range and ticks
    let vals: Vec<f64> = s.series.iter().flat_map(|x| x.data.iter().copied()).filter(|v| v.is_finite()).collect();
    let (mut lo, mut hi) = vals.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if vals.is_empty() {
        (lo, hi) = (0.0, 1.0);
    }
    if s.begin_at_zero || s.kind == "bar" {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    if hi - lo < 1e-9 {
        hi = lo + 1.0;
    }
    let step = nice_step(hi - lo, 5);
    let (lo, hi) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    let ticks: Vec<f64> = (0..=((hi - lo) / step).round() as usize).map(|i| lo + i as f64 * step).collect();
    let left = ticks.iter().map(|t| fmt_num(*t).len()).max().unwrap_or(1) as f32 * font * 0.6 + font;
    let (top, bottom) = (legend_h + font * 0.5, h - font * 2.0);
    let (x0, x1) = (left, w - font * 0.5);
    let y_of = |v: f64| bottom - ((v - lo) / (hi - lo)) as f32 * (bottom - top);
    for t in &ticks {
        let y = y_of(*t);
        out.push(Prim::Line { pts: vec![(x0, y), (x1, y)], color: grid, width: 0.6 });
        out.push(Prim::Text { x: x0 - font * 0.4, y: y - font * 0.6, text: fmt_num(*t), size: font, color: fg, align: Align::Right });
    }
    let n = s.labels.len().max(s.series.iter().map(|x| x.data.len()).max().unwrap_or(0)).max(1);
    let slot = (x1 - x0) / n as f32;
    for i in 0..n {
        let label = s.labels.get(i).cloned().unwrap_or_default();
        out.push(Prim::Text { x: x0 + slot * (i as f32 + 0.5), y: bottom + font * 0.4, text: label, size: font, color: fg, align: Align::Center });
    }
    match s.kind.as_str() {
        "bar" => {
            let k = s.series.len().max(1) as f32;
            let bw = slot * 0.8 / k;
            for (si, ser) in s.series.iter().enumerate() {
                for (i, v) in ser.data.iter().enumerate().filter(|(_, v)| v.is_finite()) {
                    let (r, g, b) = if s.label_colors && s.series.len() == 1 { PALETTE[i % PALETTE.len()] } else { PALETTE[si % PALETTE.len()] };
                    let x = x0 + slot * i as f32 + slot * 0.1 + bw * si as f32;
                    let (ya, yb) = (y_of(*v), y_of(0f64.clamp(lo, hi)));
                    out.push(Prim::Rect { x, y: ya.min(yb), w: bw, h: (yb - ya).abs(), fill: Rgba(r, g, b, 0.25), stroke: Some(Rgba(r, g, b, 1.0)) });
                }
            }
        }
        _ => {
            for (si, ser) in s.series.iter().enumerate() {
                let (r, g, b) = PALETTE[si % PALETTE.len()];
                let pts: Vec<(f32, f32)> = ser.data.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(i, v)| (x0 + slot * (i as f32 + 0.5), y_of(*v))).collect();
                if s.fill && pts.len() > 1 {
                    let base = y_of(lo.max(0f64.min(hi)));
                    let mut poly = pts.clone();
                    poly.push((pts[pts.len() - 1].0, base));
                    poly.push((pts[0].0, base));
                    out.push(Prim::Polygon { pts: poly, fill: Rgba(r, g, b, 0.25) });
                }
                out.push(Prim::Line { pts: pts.clone(), color: Rgba(r, g, b, 1.0), width: 2.0 });
                for (x, y) in &pts {
                    out.push(Prim::Rect { x: x - 2.5, y: y - 2.5, w: 5.0, h: 5.0, fill: Rgba(r, g, b, 1.0), stroke: None });
                }
            }
        }
    }
    if s.best_fit {
        if let Some(ser) = s.series.first() {
            let pts: Vec<(f64, f64)> = ser.data.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(i, v)| (i as f64, *v)).collect();
            if pts.len() > 1 {
                let m = pts.len() as f64;
                let (sx, sy) = pts.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
                let sxx: f64 = pts.iter().map(|(x, _)| x * x).sum();
                let sxy: f64 = pts.iter().map(|(x, y)| x * y).sum();
                let slope = (m * sxy - sx * sy) / (m * sxx - sx * sx);
                let icpt = (sy - slope * sx) / m;
                let at = |i: f64| (x0 + slot * (i as f32 + 0.5), y_of(icpt + slope * i));
                out.push(Prim::Line { pts: vec![at(0.0), at((n - 1) as f64)], color: Rgba(120, 120, 120, 1.0), width: 1.5 });
            }
        }
    }
    out
}

/// The editor's widest text column, in pixels (760 less its padding).
const READING_WIDTH: f32 = 696.0;

/// The primitives as Typst markup: a box of `w`×`h` points drawn with `place`.
pub fn to_typst(prims: &[Prim], w: f32, h: f32) -> String {
    let c = |c: &Rgba| format!("rgb({}, {}, {}, {}%)", c.0, c.1, c.2, (c.3 * 100.0).round());
    let esc = |t: &str| t.replace('\\', "\\\\").replace('"', "\\\"");
    let mut s = format!("box(width: {w:.1}pt, height: {h:.1}pt, {{\n");
    for p in prims {
        match p {
            // an empty bar: GPUI paints nothing for it, nor does the PDF
            Prim::Rect { h, .. } if *h < 0.5 => {}
            Prim::Rect { x, y, w, h, fill, stroke } => {
                let st = stroke.map(|k| format!(", stroke: 0.8pt + {}", c(&k))).unwrap_or_default();
                s += &format!("  place(dx: {x:.1}pt, dy: {y:.1}pt, rect(width: {w:.1}pt, height: {h:.1}pt, fill: {}{st}))\n", c(fill));
            }
            Prim::Line { pts, color, width } => {
                for seg in pts.windows(2) {
                    s += &format!("  place(line(start: ({:.1}pt, {:.1}pt), end: ({:.1}pt, {:.1}pt), stroke: {width:.1}pt + {}))\n", seg[0].0, seg[0].1, seg[1].0, seg[1].1, c(color));
                }
            }
            Prim::Polygon { pts, fill } => {
                let v: Vec<String> = pts.iter().map(|(x, y)| format!("({x:.1}pt, {y:.1}pt)")).collect();
                s += &format!("  place(polygon(fill: {}, {}))\n", c(fill), v.join(", "));
            }
            Prim::Text { x, y, text, size, color, align } => {
                let (dx, al) = match align {
                    Align::Left => (*x, "left"),
                    Align::Center => (x - 100.0, "center"),
                    Align::Right => (x - 200.0, "right"),
                };
                let bw = if *align == Align::Left { "auto".to_string() } else if *align == Align::Center { "200pt".into() } else { "200pt".into() };
                s += &format!("  place(dx: {dx:.1}pt, dy: {y:.1}pt, box(width: {bw}, align({al}, text(size: {size:.1}pt, fill: {}, \"{}\"))))\n", c(color), esc(text));
            }
        }
    }
    s += "})";
    s
}

fn hsla_of(c: &Rgba) -> Hsla {
    gpui_kit::Rgba { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a: c.3 }.into()
}

fn rgba_of(h: Hsla) -> Rgba {
    let r: gpui_kit::Rgba = h.into();
    Rgba((r.r * 255.0) as u8, (r.g * 255.0) as u8, (r.b * 255.0) as u8, r.a)
}

struct ChartBlock;

impl CodeBlockRenderer for ChartBlock {
    fn render(&self, code: &str, _md: &MarkdownRenderer, _window: &mut Window, cx: &mut App) -> AnyElement {
        let spec = match parse(code) {
            Ok(s) if s.kind == "bar" || s.kind == "line" => s,
            Ok(s) => return div().text_sm().text_color(cx.theme().muted_foreground).child(format!("Chart type \"{}\" is not supported yet (bar and line are).", s.kind)).into_any_element(),
            Err(e) => return div().text_sm().text_color(cx.theme().danger).child(format!("Chart: {e}")).into_any_element(),
        };
        let avail = crate::atoms::CONTENT_WIDTH.with(|c| c.get()).clamp(200.0, crate::atoms::MAX_IMAGE_WIDTH);
        let w = avail * spec.width;
        let h = w / 2.0;
        let theme = cx.theme();
        let (fg, grid) = (rgba_of(theme.muted_foreground), rgba_of(theme.border));
        let prims = Rc::new(layout(&spec, w, h, fg, grid));
        let font_family = theme.font_family.clone();
        div()
            .w_full()
            .flex()
            .justify_center()
            .py_2()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        let o = bounds.origin;
                        let p = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                        for prim in prims.iter() {
                            match prim {
                                Prim::Rect { x, y, w, h, fill, stroke } => {
                                    let b = Bounds::new(p(*x, *y), size(px(*w), px(*h)));
                                    let q = match stroke {
                                        Some(s) => gpui_kit::quad(b, px(0.), hsla_of(fill), px(1.), hsla_of(s), BorderStyle::Solid),
                                        None => gpui_kit::fill(b, hsla_of(fill)),
                                    };
                                    window.paint_quad(q);
                                }
                                Prim::Line { pts, color, width } => {
                                    let mut pb = PathBuilder::stroke(px(*width));
                                    if let Some((x, y)) = pts.first() {
                                        pb.move_to(p(*x, *y));
                                        for (x, y) in &pts[1..] {
                                            pb.line_to(p(*x, *y));
                                        }
                                        if let Ok(path) = pb.build() {
                                            window.paint_path(path, hsla_of(color));
                                        }
                                    }
                                }
                                Prim::Polygon { pts, fill } => {
                                    let mut pb = PathBuilder::fill();
                                    if let Some((x, y)) = pts.first() {
                                        pb.move_to(p(*x, *y));
                                        for (x, y) in &pts[1..] {
                                            pb.line_to(p(*x, *y));
                                        }
                                        pb.close();
                                        if let Ok(path) = pb.build() {
                                            window.paint_path(path, hsla_of(fill));
                                        }
                                    }
                                }
                                Prim::Text { x, y, text, size: fs, color, align } => {
                                    let run = TextRun { len: text.len(), font: font(font_family.clone()), color: hsla_of(color), background_color: None, underline: None, strikethrough: None };
                                    let line = window.text_system().shape_line(text.clone().into(), px(*fs), &[run], None);
                                    let width = f32::from(line.width);
                                    let dx = match align {
                                        Align::Left => 0.0,
                                        Align::Center => -width / 2.0,
                                        Align::Right => -width,
                                    };
                                    let _ = line.paint(p(x + dx, *y), px(fs * 1.3), TextAlign::Left, None, window, cx);
                                }
                            }
                        }
                    },
                )
                .w(px(w))
                .h(px(h)),
            )
            .into_any_element()
    }

    fn export_typst(&self, code: &str, _md: &dyn Fn(&str) -> String) -> Option<String> {
        let spec = parse(code).ok().filter(|s| s.kind == "bar" || s.kind == "line")?;
        // drawn as the editor draws it in a full-width pane (696 px of text), 1 pt per pixel,
        // then scaled to the PDF's column at its text size (`u_`: a reading-view pixel)
        let w = READING_WIDTH * spec.width;
        let h = w / 2.0;
        let chart = to_typst(&layout(&spec, w, h, Rgba(0x5c, 0x5c, 0x5c, 1.0), Rgba(0xe0, 0xe0, 0xe0, 1.0)), w, h);
        Some(format!(
            "#block(width: 100%, inset: (y: 8 * u_), align(center, layout(size => scale(calc.min(size.width, {READING_WIDTH} * u_) * {:.3} / {w:.1}pt * 100%, origin: top + left, reflow: true, {chart}))))",
            spec.width
        ))
    }
}

pub struct Charts {
    manifest: PluginManifest,
}

impl Charts {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "charts",
                name: "Charts",
                version: "0.1.0",
                description: "Draws ```chart blocks (Obsidian Charts syntax): bar and line charts, also in PDF export.",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &["chart"],
            },
        }
    }
}

impl Plugin for Charts {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        reg.code_block_renderer("chart", ChartBlock);
    }
}

#[cfg(test)]
mod tests {
    use super::{layout, parse, to_typst, Prim, Rgba};

    #[test]
    fn parses_the_plugins_syntax() {
        let s = parse("type: bar\nlabels: [Mon, \"Tue, late\", Wed]\nseries:\n  - title: A\n    data: [1, 2.5, 3]\n  - title: B\n    data: [3,2,1]\ntension: 0.2\nwidth: 80%\nbeginAtZero: true\nbestFit: false\nbestFitTitle: undefined\n").unwrap();
        assert_eq!(s.kind, "bar");
        assert_eq!(s.labels, vec!["Mon", "Tue, late", "Wed"]);
        assert_eq!(s.series.len(), 2);
        assert_eq!(s.series[0].data, vec![1.0, 2.5, 3.0]);
        assert_eq!(s.series[1].title, "B");
        assert!((s.width - 0.8).abs() < 1e-6 && s.begin_at_zero && !s.best_fit);
        let b = parse("type: line\nlabels:\n  - x\n  - y\nseries:\n  - data: [1, 2]\n").unwrap();
        assert_eq!(b.labels, vec!["x", "y"]);
        assert!(parse("labels: [a]").is_err());
        // a block indented as a whole, as most of the vault's are
        let t = parse("\ttype: bar\n\tlabels: [1, 2]\n\tseries:\n\t\t- title: s\n\t\t  data: [0, 3]\n").unwrap();
        assert_eq!((t.kind.as_str(), t.labels.len(), t.series[0].data.clone()), ("bar", 2, vec![0.0, 3.0]));
    }

    #[test]
    fn layout_draws_every_value_and_typst_has_them() {
        let s = parse("type: bar\nlabels: [a, b]\nseries:\n  - title: S\n    data: [1, 4]\n  - title: T\n    data: [2, 3]\n").unwrap();
        let p = layout(&s, 400.0, 200.0, Rgba(0, 0, 0, 1.0), Rgba(200, 200, 200, 1.0));
        let bars: Vec<&Prim> = p.iter().filter(|x| matches!(x, Prim::Rect { stroke: Some(_), w, .. } if *w > 20.0)).collect();
        assert_eq!(bars.len(), 4, "two series × two labels");
        // taller value, taller bar
        let height = |i: usize| if let Prim::Rect { h, .. } = bars[i] { *h } else { 0.0 };
        assert!(height(1) > height(0));
        let t = to_typst(&p, 400.0, 200.0);
        assert!(t.starts_with("box(width: 400.0pt") && t.contains("rect(") && t.contains("\"a\""));
        let l = parse("type: line\nlabels: [a, b, c]\nseries:\n  - title: S\n    data: [1, 2, 3]\nfill: true\nbestFit: true\n").unwrap();
        let p = layout(&l, 400.0, 200.0, Rgba(0, 0, 0, 1.0), Rgba(200, 200, 200, 1.0));
        assert!(p.iter().any(|x| matches!(x, Prim::Polygon { .. })), "fill");
        assert!(to_typst(&p, 400.0, 200.0).contains("polygon("));
    }
}
