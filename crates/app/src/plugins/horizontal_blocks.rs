//! Horizontal blocks: a reimplementation of the community plugin "Horizontal Blocks"
//! (iCodeAlchemy/horizontal-blocks 1.1.1, MIT), from its source. A ```` ```horizontal ```` or
//! ```` ```hblock ```` fence is split at lines that are exactly `---` (outside fenced code) into
//! side-by-side columns of rendered Markdown:
//!
//! - a divider between columns can be dragged to resize them; the left column keeps its width
//!   (in px), the right one takes the rest;
//! - widths and per-column colours are saved by block, keyed like the original
//!   (`horizontal-block-layout-<first 16 hex of SHA-256(source)>` → `width-<i>`, `bg-<i>`,
//!   `fg-<i>`, `title-<i>`), so layouts saved by Obsidian apply here. Obsidian's
//!   `.obsidian/plugins/horizontal-blocks/data.json` is read, never written: ours go to
//!   `.tephra/horizontal-blocks.json` and win over Obsidian's for the same key;
//! - the styling settings (divider, blocks, spacing, toolbar, theme-aware colours) are the
//!   original's, with its defaults, or its saved `style` when Obsidian has one;
//! - each column has a toolbar (hidden by default) with background/text colour pickers and
//!   width −/+; right-clicking a column toggles the toolbar, borders and alternating shading.
//!
//! Differences: clicking a column puts the caret in the block (its source shows), as for every
//! rendered block here; embedded notes inside a column render read-only (the original makes them
//! editable in place); after a drag the right column's saved width is dropped, so the layout
//! stays as dragged (the original keeps it and re-applies it on the next render).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::api::*;
use super::editing_toolbar::hex;
use super::PluginHost;

const ID: &str = "horizontal-blocks";
const LAYOUT_PREFIX: &str = "horizontal-block-layout-";
/// The toolbar's width step and smallest width (px), as in the original.
const STEP: f32 = 32.0;
const MIN_TOOLBAR_WIDTH: f32 = 80.0;
/// Narrowest a column can be dragged to.
const MIN_DRAG_WIDTH: f32 = 40.0;

// ---- parsing ----------------------------------------------------------------------------------

/// The block's text as Obsidian hands it to the plugin: `\r\n` line ends become `\n` (the
/// editor gives the text between the fences without the last line end, as Obsidian does).
fn normalise(code: &str) -> String {
    code.replace("\r\n", "\n")
}

/// Splits a block into columns at lines that are exactly `---` (ignoring surrounding spaces),
/// except inside fenced code; each column is trimmed. Same rules as the original's
/// `splitSections`: a fence opens with 3+ backticks or tildes and closes with at least as many
/// of the same character.
pub fn split_sections(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in src.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let t = line.trim_start();
        let run = t.chars().next().filter(|c| *c == '`' || *c == '~').map(|c| (c, t.chars().take_while(|x| *x == c).count())).filter(|(_, n)| *n >= 3);
        match (fence, run) {
            (None, Some(r)) => fence = Some(r),
            (Some((c, n)), Some((rc, rn))) if c == rc && rn >= n => fence = None,
            _ => {}
        }
        if fence.is_none() && line.trim() == "---" {
            out.push(cur.join("\n").trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(line);
    }
    out.push(cur.join("\n").trim().to_string());
    out
}

/// The original's block id: the first 16 hex digits of SHA-256 of the block's text.
pub fn block_id(src: &str) -> String {
    Sha256::digest(src.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

// ---- styling settings -------------------------------------------------------------------------

/// The original's `StyleSettings` (same keys, same defaults).
#[derive(Clone, Debug, PartialEq)]
struct Style {
    theme_aware: bool,
    divider_color: String,
    divider_thickness: f32,
    divider_opacity: f32,
    divider_style: String,
    block_bg_color: String,
    block_text_color: String,
    title_text_color: String,
    alternating_shading: bool,
    show_borders: bool,
    border_radius: f32,
    border_thickness: f32,
    transparent_background: bool,
    block_padding: f32,
    block_gap: f32,
    show_toolbar: bool,
    divider_hover_color: String,
}

/// (key, default) of every styling setting, in the original's order.
fn style_defaults() -> Vec<(&'static str, Value)> {
    vec![
        ("themeAware", json!(true)),
        ("dividerColor", json!("#6aa9ff")),
        ("dividerThickness", json!(2)),
        ("dividerOpacity", json!(1)),
        ("dividerStyle", json!("solid")),
        ("blockBgColor", json!("#2b2b2b")),
        ("blockTextColor", json!("#e0e0e0")),
        ("titleTextColor", json!("#7aa2ff")),
        ("alternatingShading", json!(false)),
        ("showBorders", json!(false)),
        ("borderRadius", json!(4)),
        ("borderThickness", json!(2)),
        ("transparentBackground", json!(false)),
        ("blockPadding", json!(12)),
        ("blockGap", json!(0)),
        ("showToolbar", json!(false)),
        ("dividerHoverColor", json!("#8bbdff")),
        ("dragActiveShadow", json!("rgba(0,0,0,0.08)")),
    ]
}

/// The colour settings, edited with colour pickers in the plugin's settings section.
const COLOR_KEYS: &[(&str, &str)] = &[
    ("dividerColor", "Divider colour"),
    ("dividerHoverColor", "Divider hover colour"),
    ("blockBgColor", "Block background"),
    ("blockTextColor", "Block text colour"),
    ("titleTextColor", "Title text colour"),
];

impl Style {
    fn from_settings(s: &Map<String, Value>) -> Self {
        let d: Map<String, Value> = style_defaults().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let get = |k: &str| s.get(k).filter(|v| !v.is_null()).or_else(|| d.get(k)).cloned().unwrap_or(Value::Null);
        let b = |k: &str| get(k).as_bool().unwrap_or(false);
        let n = |k: &str| get(k).as_f64().unwrap_or(0.0) as f32;
        let t = |k: &str| get(k).as_str().unwrap_or("").to_string();
        Self {
            theme_aware: b("themeAware"),
            divider_color: t("dividerColor"),
            divider_thickness: n("dividerThickness").clamp(1.0, 5.0),
            divider_opacity: n("dividerOpacity").clamp(0.0, 1.0),
            divider_style: t("dividerStyle"),
            block_bg_color: t("blockBgColor"),
            block_text_color: t("blockTextColor"),
            title_text_color: t("titleTextColor"),
            alternating_shading: b("alternatingShading"),
            show_borders: b("showBorders"),
            border_radius: n("borderRadius").max(0.0),
            border_thickness: n("borderThickness").max(0.0),
            transparent_background: b("transparentBackground"),
            block_padding: n("blockPadding").max(0.0),
            block_gap: n("blockGap").max(0.0),
            show_toolbar: b("showToolbar"),
            divider_hover_color: t("dividerHoverColor"),
        }
    }
}

/// The colours one render uses (theme-aware ones follow the app's theme).
struct Colors {
    divider: Hsla,
    hover: Hsla,
    bg: Hsla,
    text: Hsla,
    title: Hsla,
    border: Hsla,
    title_bg: Hsla,
}

fn colors(s: &Style, cx: &App) -> Colors {
    let th = cx.theme();
    let pick = |own: &str, theme: Hsla| if s.theme_aware { theme } else { crate::css::parse_color(own).unwrap_or(theme) };
    let bg = if s.transparent_background { transparent_black() } else { pick(&s.block_bg_color, th.secondary) };
    Colors {
        divider: pick(&s.divider_color, th.primary),
        hover: pick(&s.divider_hover_color, th.link),
        bg,
        text: pick(&s.block_text_color, th.foreground),
        title: pick(&s.title_text_color, th.link),
        border: th.border,
        title_bg: th.accent,
    }
}

/// `c` with `amount` of black mixed in (the original's alternating shading: 6%).
fn darken(c: Hsla, amount: f32) -> Hsla {
    let r = Rgba::from(c);
    let k = 1.0 - amount;
    Hsla::from(Rgba { r: r.r * k, g: r.g * k, b: r.b * k, a: r.a })
}

// ---- saved layouts ----------------------------------------------------------------------------

/// Layouts by block (Obsidian's `data.json` shape): Obsidian's entries, then ours on top.
struct Layouts {
    merged: Map<String, Value>,
    ours: Map<String, Value>,
    file: Option<PathBuf>,
}

fn read_json(p: &Path) -> Option<Map<String, Value>> {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned())
}

fn obsidian_data(vault: &Path) -> Map<String, Value> {
    read_json(&vault.join(".obsidian/plugins/horizontal-blocks/data.json")).unwrap_or_default()
}

impl Layouts {
    fn load(vault: &Path) -> Self {
        let file = vault.join(".tephra/horizontal-blocks.json");
        let ours = read_json(&file).unwrap_or_default();
        Self::from_parts(obsidian_data(vault), ours, Some(file))
    }

    fn from_parts(obsidian: Map<String, Value>, ours: Map<String, Value>, file: Option<PathBuf>) -> Self {
        let mut merged: Map<String, Value> = obsidian.into_iter().filter(|(k, v)| k.starts_with(LAYOUT_PREFIX) && v.is_object()).collect();
        for (k, v) in &ours {
            let Some(o) = v.as_object() else { continue };
            let entry = merged.entry(k.clone()).or_insert_with(|| json!({}));
            if let Some(e) = entry.as_object_mut() {
                for (p, x) in o {
                    e.insert(p.clone(), x.clone());
                }
            }
        }
        Self { merged, ours, file }
    }

    fn get(&self, id: &str) -> Map<String, Value> {
        self.merged.get(&format!("{LAYOUT_PREFIX}{id}")).and_then(Value::as_object).cloned().unwrap_or_default()
    }

    /// Sets (or with `Null`, removes) one property of a block's layout, and saves.
    fn set(&mut self, id: &str, prop: &str, value: Value) {
        let key = format!("{LAYOUT_PREFIX}{id}");
        for map in [&mut self.merged, &mut self.ours] {
            let entry = map.entry(key.clone()).or_insert_with(|| json!({}));
            if let Some(e) = entry.as_object_mut() {
                // a removal is recorded in ours as null, so Obsidian's value stays hidden
                e.insert(prop.to_string(), value.clone());
            }
        }
        self.save();
    }

    /// Removes the per-block colours (the original's "Reset styling", which keeps widths).
    fn clear_colors(&mut self) {
        let keys: Vec<(String, String)> = self
            .merged
            .iter()
            .filter_map(|(k, v)| v.as_object().map(|o| (k, o)))
            .flat_map(|(k, o)| o.keys().filter(|p| p.starts_with("bg-") || p.starts_with("fg-")).map(move |p| (k.clone(), p.clone())))
            .collect();
        for (k, p) in keys {
            if let Some(id) = k.strip_prefix(LAYOUT_PREFIX) {
                let id = id.to_string();
                for map in [&mut self.merged, &mut self.ours] {
                    if let Some(e) = map.entry(format!("{LAYOUT_PREFIX}{id}")).or_insert_with(|| json!({})).as_object_mut() {
                        e.insert(p.clone(), Value::Null);
                    }
                }
            }
        }
        self.save();
    }

    fn save(&self) {
        let Some(p) = &self.file else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tephra-tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&Value::Object(self.ours.clone())).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

/// A saved width (px); null or missing = flexible.
fn width_of(layout: &Map<String, Value>, i: usize) -> Option<f32> {
    layout.get(&format!("width-{i}")).and_then(Value::as_f64).filter(|w| *w > 0.0).map(|w| w as f32)
}

fn color_of(layout: &Map<String, Value>, prop: &str, i: usize) -> Option<Hsla> {
    layout.get(&format!("{prop}-{i}")).and_then(Value::as_str).and_then(crate::css::parse_color)
}

fn title_of(layout: &Map<String, Value>, i: usize) -> Option<String> {
    layout.get(&format!("title-{i}")).and_then(Value::as_str).filter(|t| !t.is_empty()).map(String::from)
}

/// The columns' widths for one render: saved ones, with the divider being dragged applied.
fn column_widths(layout: &Map<String, Value>, n: usize, drag: Option<&Drag>) -> Vec<Option<f32>> {
    (0..n)
        .map(|i| match drag {
            Some(d) if d.index == i => Some(d.width),
            Some(d) if d.index + 1 == i => None,
            _ => width_of(layout, i),
        })
        .collect()
}

// ---- the renderer -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Drag {
    id: String,
    index: usize,
    start_x: f32,
    start_w: f32,
    width: f32,
    /// Widest the column may get (the container minus room for the others).
    max_w: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Bg,
    Fg,
}

struct Picker {
    state: Entity<ColorPickerState>,
    _subs: Vec<Subscription>,
}

struct Shared {
    style: Style,
    layouts: RefCell<Layouts>,
    drag: RefCell<Option<Drag>>,
    /// Last frame's widths: of each column by (block, index), of each block by id.
    col_w: RefCell<HashMap<(String, usize), f32>>,
    pickers: RefCell<HashMap<(String, usize, Kind), Picker>>,
}

impl Shared {
    fn set_layout(&self, id: &str, prop: &str, value: Value) {
        self.layouts.borrow_mut().set(id, prop, value);
    }

    /// The colour picker of one column (created on first use, kept while the plugin runs).
    fn picker(self: &Rc<Self>, id: &str, i: usize, kind: Kind, current: Hsla, window: &mut Window, cx: &mut App) -> Entity<ColorPickerState> {
        let key = (id.to_string(), i, kind);
        if let Some(p) = self.pickers.borrow().get(&key) {
            return p.state.clone();
        }
        let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(current));
        let (me, id) = (Rc::downgrade(self), id.to_string());
        let prop = if kind == Kind::Bg { "bg" } else { "fg" };
        let sub = window.subscribe(&state, cx, move |_, ev: &ColorPickerEvent, window, _| {
            let ColorPickerEvent::Change(Some(c)) = ev else { return };
            if let Some(me) = me.upgrade() {
                me.set_layout(&id, &format!("{prop}-{i}"), Value::from(hex(*c)));
                window.refresh();
            }
        });
        self.pickers.borrow_mut().insert(key, Picker { state: state.clone(), _subs: vec![sub] });
        state
    }
}

struct Columns(Rc<Shared>);

/// Element ids inside one block (the editor gives each rendered block its own id scope).
fn eid(what: &str, i: usize) -> ElementId {
    ElementId::Name(SharedString::from(format!("hblock-{what}-{i}")))
}

thread_local! {
    /// Each block's laid-out width by id (last frame). Kept across plugin reloads, so a
    /// settings change doesn't lay columns out from a guess.
    static BLOCK_W: RefCell<HashMap<String, f32>> = RefCell::default();
}

/// A top-level block's width before it has been drawn: the editor's text column (the pane
/// minus its padding, at most the readable line width; see `Editor::render`).
fn editor_text_width(cx: &App) -> f32 {
    let scale = crate::theme::ui_scale(cx);
    crate::atoms::CONTENT_WIDTH.with(|w| w.get()).min((760.0 - 64.0) * scale)
}

/// The text width inside each column of a block `total` wide: fixed columns have their width,
/// flexible ones share what is left after the dividers, the gaps between all of them and the
/// columns' 2 px side margins; then each loses its padding and borders.
fn text_widths(total: f32, widths: &[Option<f32>], s: &Style) -> Vec<f32> {
    let n = widths.len().max(1) as f32;
    let fixed: f32 = widths.iter().flatten().sum();
    let flex_n = widths.iter().filter(|w| w.is_none()).count().max(1) as f32;
    let between = (n - 1.0) * s.divider_thickness + (2.0 * n - 2.0) * s.block_gap + 4.0 * n;
    let flex_w = ((total - fixed - between) / flex_n).max(0.0);
    let chrome = 2.0 * s.block_padding + if s.show_borders { 2.0 * s.border_thickness } else { 0.0 };
    widths.iter().map(|w| (w.unwrap_or(flex_w) - chrome).max(32.0)).collect()
}

/// Lays out its child with the editor's "content width" (what images shrink to) set to `width`:
/// a column is narrower than the pane. Images are sized while the Markdown lays out, so the
/// width has to be in place then, not only while the column is built.
struct ContentWidth {
    width: f32,
    child: Option<AnyElement>,
}

impl ContentWidth {
    fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        let prev = crate::atoms::CONTENT_WIDTH.with(|c| c.replace(self.width));
        let r = f();
        crate::atoms::CONTENT_WIDTH.with(|c| c.set(prev));
        r
    }
}

impl IntoElement for ContentWidth {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ContentWidth {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().expect("child");
        let id = self.with(|| child.request_layout(window, cx));
        let layout = window.request_layout(gpui_kit::Style { size: Size { width: relative(1.).into(), height: Length::Auto }, ..Default::default() }, [id], cx);
        (layout, child)
    }
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, child: &mut AnyElement, window: &mut Window, cx: &mut App) {
        self.with(|| child.prepaint(window, cx));
    }
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, child: &mut AnyElement, _: &mut (), window: &mut Window, cx: &mut App) {
        self.with(|| child.paint(window, cx));
    }
}

/// A zero-size element that records its parent's laid-out width.
/// `f` records the width and returns the previous one: when it changed (a block nested in a
/// callout or list, drawn for the first time; a resized pane), another frame lays it out again.
fn measure(f: impl FnOnce(f32) -> Option<f32> + 'static) -> impl IntoElement {
    canvas(
        move |b, window, _| {
            let w = f32::from(b.size.width);
            if f(w).is_none_or(|prev| (prev - w).abs() > 0.5) {
                // a refresh asked for while drawing is dropped: ask for it on the next frame
                window.on_next_frame(|window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl CodeBlockRenderer for Columns {
    fn render(&self, code: &str, md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> AnyElement {
        let sh = &self.0;
        let src = normalise(code);
        let id = block_id(&src);
        let sections = split_sections(&src);
        let n = sections.len();
        let layout = sh.layouts.borrow().get(&id);
        let drag = sh.drag.borrow().clone().filter(|d| d.id == id);
        let widths = column_widths(&layout, n, drag.as_ref());
        let s = &sh.style;
        let c = colors(s, cx);

        // each column's text width, for images (they shrink to the width they are given)
        // each column's text width, worked out before layout: images are sized from it, and a
        // width measured afterwards would draw them at the wrong size for a frame
        let measured = BLOCK_W.with(|m| m.borrow().get(&id).copied());
        let total = measured.unwrap_or_else(|| editor_text_width(cx));
        let text_ws = text_widths(total, &widths, s);

        let mut children: Vec<AnyElement> = Vec::with_capacity(2 * n);
        for (i, section) in sections.iter().enumerate() {
            let text_w = text_ws[i];
            let body = ContentWidth { width: text_w, child: None }.with(|| md.render(section, window, cx));
            let body = ContentWidth { width: text_w, child: Some(body) };

            let mut bg = color_of(&layout, "bg", i).unwrap_or(c.bg);
            if s.alternating_shading && i % 2 == 0 {
                bg = darken(bg, 0.06);
            }
            let fg = color_of(&layout, "fg", i).unwrap_or(c.text);
            let rec = (Rc::downgrade(sh), id.clone());
            let mut col = div()
                .id(eid("col", i))
                .relative()
                .flex()
                .flex_col()
                .min_w_0()
                .overflow_hidden()
                .mx(px(2.))
                .rounded(px(s.border_radius))
                .bg(bg)
                .child(measure(move |w| {
                    if let Some(me) = rec.0.upgrade() {
                        me.col_w.borrow_mut().insert((rec.1, i), w);
                    }
                    Some(w)
                }))
                .children(crate::script::probe(|| format!("hblock-col:{i}"), cx));
            col = match widths[i] {
                Some(w) => col.flex_none().w(px(w)),
                None => col.flex_1(),
            };
            if s.show_borders && s.border_thickness > 0.0 {
                let mut st = col.style().border_widths.clone();
                let b: AbsoluteLength = px(s.border_thickness).into();
                st.top = Some(b);
                st.right = Some(b);
                st.bottom = Some(b);
                st.left = Some(b);
                col.style().border_widths = st;
                col = col.border_color(c.border);
            }
            if let Some(t) = title_of(&layout, i) {
                col = col.child(
                    div().px(px(12.)).pt(px(4.)).pb(px(6.)).text_sm().font_weight(FontWeight::SEMIBOLD).text_color(c.title).bg(c.title_bg).border_b_1().border_color(c.divider).child(t),
                );
            }
            col = col.child(div().flex_1().min_w_0().p(px(s.block_padding)).text_color(fg).child(body));
            if s.show_toolbar {
                col = col.child(self.toolbar(&id, i, bg, fg, window, cx));
            }
            children.push(self.with_menu(col).into_any_element());
            if i + 1 < n {
                children.push(self.divider(&id, i, &c, cx).into_any_element());
            }
        }

        let bid = id.clone();
        let mut root = div()
            .relative()
            .flex()
            .flex_row()
            .w_full()
            .min_h(px(50.))
            .rounded(px(6.))
            .overflow_hidden()
            .shadow_sm()
            .gap(px(s.block_gap))
            .child(measure(move |w| BLOCK_W.with(|m| m.borrow_mut().insert(bid, w))))
            .children(children);
        if drag.is_some() {
            root = root.child(self.drag_listener());
        }
        if measured.is_none() {
            // never drawn: its width is a guess (the pane's width isn't known on the app's first
            // frame, a block in a callout is narrower), so it is laid out unseen for one frame
            // rather than shown with images at the wrong size
            root = root.invisible();
        }
        root.into_any_element()
    }

    fn export_typst(&self, code: &str, md: &dyn Fn(&str) -> String) -> Option<String> {
        Some(self.typst(code, md))
    }
}

impl Columns {
    /// The draggable divider after column `i`: its visible line is `dividerThickness` wide; the
    /// area that takes the mouse is a few pixels wider on each side.
    fn divider(&self, id: &str, i: usize, c: &Colors, cx: &App) -> impl IntoElement {
        let s = &self.0.style;
        let t = s.divider_thickness;
        let dragging = self.0.drag.borrow().as_ref().is_some_and(|d| d.id == id && d.index == i);
        let group = SharedString::from(format!("hblock-divider-{i}"));
        let mut line = div().absolute().top_0().bottom_0().left(px(4.)).w(px(t)).opacity(s.divider_opacity);
        line = match s.divider_style.as_str() {
            "transparent" => line,
            "dashed" | "dotted" => {
                let mut l = line.border_dashed();
                let b: AbsoluteLength = px(t).into();
                l.style().border_widths.left = Some(b);
                l.border_color(if dragging { c.hover } else { c.divider }).group_hover(group.clone(), |st| st.border_color(c.hover))
            }
            _ => line.bg(if dragging { c.hover } else { c.divider }).group_hover(group.clone(), |st| st.bg(c.hover)),
        };
        let me = Rc::downgrade(&self.0);
        let id = id.to_string();
        let hit = div()
            .id(eid("divider", i))
            .group(group)
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-4.))
            .w(px(t + 8.))
            .cursor_col_resize()
            .child(line)
            .children(crate::script::probe(|| format!("hblock-divider:{i}"), cx))
            .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                // not the block's own click (which would show its source)
                cx.stop_propagation();
                let Some(me) = me.upgrade() else { return };
                // the divider trades width between its two neighbours
                let w = |k: usize| me.col_w.borrow().get(&(id.clone(), k)).copied();
                let start_w = w(i).unwrap_or(200.0);
                let max_w = (start_w + w(i + 1).unwrap_or(0.0) - MIN_DRAG_WIDTH).max(MIN_DRAG_WIDTH);
                *me.drag.borrow_mut() = Some(Drag { id: id.clone(), index: i, start_x: f32::from(ev.position.x), start_w, width: start_w, max_w });
                window.refresh();
            });
        div().relative().flex_none().w(px(t)).child(hit)
    }

    /// While a divider is dragged: follows the mouse anywhere in the window and saves on release.
    fn drag_listener(&self) -> impl IntoElement {
        let me = Rc::downgrade(&self.0);
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.set_window_cursor_style(CursorStyle::ResizeLeftRight);
                let m = me.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let Some(me) = m.upgrade() else { return };
                    if ev.pressed_button != Some(MouseButton::Left) {
                        // the button went up outside the window
                        finish_drag(&me);
                    } else if let Some(d) = me.drag.borrow_mut().as_mut() {
                        d.width = (d.start_w + f32::from(ev.position.x) - d.start_x).clamp(MIN_DRAG_WIDTH, d.max_w.max(MIN_DRAG_WIDTH));
                    }
                    window.refresh();
                });
                let m = me.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                        return;
                    }
                    if let Some(me) = m.upgrade() {
                        finish_drag(&me);
                    }
                    cx.stop_propagation();
                    window.refresh();
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The per-column toolbar: background and text colour, width −/+.
    fn toolbar(&self, id: &str, i: usize, bg: Hsla, fg: Hsla, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let th = cx.theme().clone();
        let bg_picker = self.0.picker(id, i, Kind::Bg, bg, window, cx);
        let fg_picker = self.0.picker(id, i, Kind::Fg, fg, window, cx);
        let button = |label: &'static str, delta: f32| {
            let (me, id) = (Rc::downgrade(&self.0), id.to_string());
            div()
                .id(eid(if delta < 0.0 { "narrower" } else { "wider" }, i))
                .px(px(4.))
                .py(px(2.))
                .rounded(px(4.))
                .text_xs()
                .text_color(th.muted_foreground)
                .cursor_pointer()
                .hover(|st| st.bg(th.list_hover).text_color(th.foreground))
                .child(label)
                .on_click(move |_, window, _| {
                    let Some(me) = me.upgrade() else { return };
                    let cur = me.col_w.borrow().get(&(id.clone(), i)).copied().unwrap_or(200.0);
                    let w = (cur + delta).round().max(MIN_TOOLBAR_WIDTH);
                    me.set_layout(&id, &format!("width-{i}"), json!(w));
                    window.refresh();
                })
        };
        div()
            .id(eid("toolbar", i))
            .absolute()
            .top(px(6.))
            .right(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(4.))
            .py(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(th.border)
            .bg(th.background)
            .opacity(0.8)
            .hover(|st| st.opacity(1.0))
            // clicks here are the toolbar's, not the block's (which would show its source)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ColorPicker::new(&bg_picker).xsmall().accessibility_label("Block background"))
            .child(ColorPicker::new(&fg_picker).xsmall().accessibility_label("Text color"))
            .child(button("-", -STEP))
            .child(button("+", STEP))
    }

    /// Right-click on a column: the original's visibility toggles (they change the settings).
    fn with_menu(&self, col: Stateful<Div>) -> impl IntoElement {
        let s = self.0.style.clone();
        col.context_menu(move |menu, _, _| {
            let toggle = |label: String, key: &'static str, now: bool| {
                PopupMenuItem::new(label).on_click(move |_, _, cx| PluginHost::set_settings(cx, ID, vec![(key, Value::Bool(!now))]))
            };
            menu.item(toggle(format!("{} toolbar", if s.show_toolbar { "Hide" } else { "Show" }), "showToolbar", s.show_toolbar))
                .item(toggle(format!("{} borders", if s.show_borders { "Hide" } else { "Show" }), "showBorders", s.show_borders))
                .item(toggle(format!("{} alternating shading", if s.alternating_shading { "Disable" } else { "Enable" }), "alternatingShading", s.alternating_shading))
        })
    }

    /// The block in PDF export: a Typst grid with the columns' widths, backgrounds and padding,
    /// and the dividers as lines (`u_` = one reading-view pixel). Theme-aware colours are the
    /// light theme's, as the export draws everything on white.
    fn typst(&self, code: &str, md: &dyn Fn(&str) -> String) -> String {
        let s = &self.0.style;
        let src = normalise(code);
        let id = block_id(&src);
        let sections = split_sections(&src);
        let n = sections.len();
        let layout = self.0.layouts.borrow().get(&id);
        let own = |v: &str| crate::css::parse_color(v).map(typst_rgb);
        let theme_or = |own_v: &str, theme: &str| if s.theme_aware { theme.to_string() } else { own(own_v).unwrap_or_else(|| theme.to_string()) };
        let bg = if s.transparent_background { "none".to_string() } else { theme_or(&s.block_bg_color, "rgb(246, 246, 246)") };
        let text = theme_or(&s.block_text_color, "c_.text");
        let title = theme_or(&s.title_text_color, "c_.accent");
        let divider = theme_or(&s.divider_color, "c_.accent");
        let divider = if s.divider_opacity < 1.0 { format!("color.mix(({divider}, {:.0}%), (white, {:.0}%))", s.divider_opacity * 100.0, (1.0 - s.divider_opacity) * 100.0) } else { divider };
        let t = s.divider_thickness;

        let mut columns = Vec::new();
        let mut fills = Vec::new();
        let mut strokes = Vec::new();
        let mut cells = Vec::new();
        for (i, section) in sections.iter().enumerate() {
            columns.push(width_of(&layout, i).map(|w| format!("{w:.0} * u_")).unwrap_or_else(|| "1fr".into()));
            let mut fill = layout.get(&format!("bg-{i}")).and_then(Value::as_str).and_then(own).unwrap_or_else(|| bg.clone());
            if s.alternating_shading && i % 2 == 0 && fill != "none" {
                fill = format!("{fill}.darken(6%)");
            }
            fills.push(fill);
            strokes.push(if s.show_borders && s.border_thickness > 0.0 { format!("{:.1} * u_ + rgb(224, 224, 224)", s.border_thickness) } else { "none".into() });
            let fg = layout.get(&format!("fg-{i}")).and_then(Value::as_str).and_then(own).unwrap_or_else(|| text.clone());
            let head = title_of(&layout, i).map(|h| format!("#block(width: 100%, below: 6 * u_)[#text(weight: 600, fill: {title})[{}]]\n", crate::export::typst::lit(&h))).unwrap_or_default();
            cells.push(format!("[\n#set text(fill: {fg})\n{head}{}\n]", md(section)));
            if i + 1 < n {
                columns.push(format!("{t:.1} * u_"));
                fills.push("none".into());
                strokes.push(match s.divider_style.as_str() {
                    "transparent" => "none".into(),
                    "dashed" | "dotted" => format!("(left: (paint: {divider}, thickness: {t:.1} * u_, dash: \"{}\"))", s.divider_style),
                    _ => format!("(left: {t:.1} * u_ + {divider})"),
                });
                cells.push("[]".into());
            }
        }
        let pad = s.block_padding;
        format!(
            "#block(width: 100%, inset: (x: 2 * u_), radius: 6 * u_, clip: true, grid(\n  columns: ({}),\n  column-gutter: {:.1} * u_,\n  fill: (x, y) => ({},).at(x),\n  stroke: (x, y) => ({},).at(x),\n  inset: (x, y) => if calc.even(x) {{ {pad:.1} * u_ }} else {{ 0pt }},\n  {}\n))",
            columns.join(", "),
            2.0 + s.block_gap,
            fills.join(", "),
            strokes.join(", "),
            cells.join(",\n  "),
        )
    }
}

fn typst_rgb(c: Hsla) -> String {
    let r = Rgba::from(c);
    let b = |x: f32| (x.clamp(0., 1.) * 255.).round() as u8;
    format!("rgb({}, {}, {})", b(r.r), b(r.g), b(r.b))
}

/// Ends a drag: the dragged column keeps its width, the one after it becomes flexible.
fn finish_drag(me: &Shared) {
    let Some(d) = me.drag.borrow_mut().take() else { return };
    if (d.width - d.start_w).abs() < 0.5 {
        return; // a click on the divider, not a drag
    }
    let w = (d.width * 100.0).round() / 100.0;
    let mut l = me.layouts.borrow_mut();
    l.set(&d.id, &format!("width-{}", d.index), json!(w));
    l.set(&d.id, &format!("width-{}", d.index + 1), Value::Null);
}

// ---- the plugin -------------------------------------------------------------------------------

pub struct HorizontalBlocks {
    manifest: PluginManifest,
    vault: PathBuf,
}

impl HorizontalBlocks {
    pub fn new(vault: &Path) -> Self {
        // the original's styling, from Obsidian when it has saved some
        let saved = obsidian_data(vault).get("style").and_then(Value::as_object).cloned().unwrap_or_default();
        let default = |k: &str, d: Value| saved.get(k).filter(|v| std::mem::discriminant(*v) == std::mem::discriminant(&d) || (v.is_number() && d.is_number())).cloned().unwrap_or(d);
        let spec = |key: &'static str, label: &'static str, description: &'static str, kind: SettingKind| {
            let d = style_defaults().into_iter().find(|(k, _)| *k == key).map(|(_, v)| v).unwrap_or(Value::Null);
            SettingSpec { key, label, description, kind, default: default(key, d) }
        };
        let styles = vec![("solid", "Solid"), ("dashed", "Dashed"), ("dotted", "Dotted"), ("transparent", "Transparent")];
        let mut settings = vec![
            spec("themeAware", "Theme-aware colors", "Use the theme's colours instead of the ones picked below.", SettingKind::Toggle),
            spec("dividerThickness", "Divider thickness", "Px, 1–5.", SettingKind::Number),
            spec("dividerOpacity", "Divider opacity", "0–1.", SettingKind::Number),
            spec("dividerStyle", "Divider style", "", SettingKind::Choice(styles)),
            spec("alternatingShading", "Alternating shading", "Zebra-style subtle alternating backgrounds.", SettingKind::Toggle),
            spec("showBorders", "Show borders", "", SettingKind::Toggle),
            spec("borderRadius", "Border radius", "Px.", SettingKind::Number),
            spec("borderThickness", "Border thickness", "Px.", SettingKind::Number),
            spec("transparentBackground", "Transparent background", "Make block backgrounds transparent.", SettingKind::Toggle),
            spec("blockPadding", "Inner padding", "Padding inside each block (px).", SettingKind::Number),
            spec("blockGap", "Gap between blocks", "Px.", SettingKind::Number),
            spec("showToolbar", "Toolbar visibility", "Show each block's toolbar (colours, width −/+). Right-click a block for this and the border/shading toggles.", SettingKind::Toggle),
            spec("dragActiveShadow", "Drag active highlight", "", SettingKind::Hidden),
        ];
        for (key, label) in COLOR_KEYS {
            settings.push(spec(key, label, "", SettingKind::Hidden));
        }
        Self {
            manifest: PluginManifest {
                id: ID,
                name: "Horizontal blocks",
                version: "1.1.1",
                description: "Side-by-side, resizable columns from ```horizontal / ```hblock blocks split at --- lines (compatible with the Horizontal Blocks community plugin).",
                enabled_by_default: false,
                settings,
                code_languages: &["horizontal", "hblock"],
            },
            vault: vault.to_path_buf(),
        }
    }
}

impl Plugin for HorizontalBlocks {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let shared = Rc::new(Shared {
            style: Style::from_settings(settings),
            layouts: RefCell::new(Layouts::load(&self.vault)),
            drag: RefCell::new(None),
            col_w: RefCell::default(),
            pickers: RefCell::default(),
        });
        for lang in ["horizontal", "hblock"] {
            reg.code_block_renderer(lang, Columns(shared.clone()));
        }
        let sh = shared.clone();
        reg.settings_tab(SettingsTab { id: "colors".into(), title: "Colours".into(), render: Rc::new(move |window, cx| settings_colors(&sh, window, cx)) });
    }
}

// ---- settings: colours and reset ---------------------------------------------------------------

/// The colour pickers of the settings section; kept across plugin reloads (each change reloads
/// the plugin), so an open picker stays open.
struct SettingPickers {
    states: HashMap<&'static str, Entity<ColorPickerState>>,
    _subs: Vec<Subscription>,
}
impl Global for SettingPickers {}

fn setting_color(key: &str, cx: &App) -> Option<Hsla> {
    cx.try_global::<PluginHost>().and_then(|h| h.setting(ID, key).as_str().and_then(crate::css::parse_color))
}

fn setting_picker(key: &'static str, window: &mut Window, cx: &mut App) -> Entity<ColorPickerState> {
    if let Some(s) = cx.try_global::<SettingPickers>().and_then(|p| p.states.get(key)) {
        return s.clone();
    }
    let init = setting_color(key, cx);
    let state = cx.new(|cx| {
        let s = ColorPickerState::new(window, cx);
        match init {
            Some(c) => s.default_value(c),
            None => s,
        }
    });
    // the sliders change the colour continuously: apply it once the picker closes
    let pending: Rc<std::cell::Cell<Option<Hsla>>> = Rc::default();
    let p = pending.clone();
    let mut subs = vec![window.subscribe(&state, cx, move |state, ev: &ColorPickerEvent, _, cx| {
        let ColorPickerEvent::Change(Some(c)) = ev else { return };
        if state.read(cx).is_open() {
            p.set(Some(*c));
        } else {
            p.set(None);
            PluginHost::set_settings(cx, ID, vec![(key, Value::from(hex(*c)))]);
        }
    })];
    subs.push(window.observe(&state, cx, move |state, _, cx| {
        if !state.read(cx).is_open() {
            if let Some(c) = pending.take() {
                PluginHost::set_settings(cx, ID, vec![(key, Value::from(hex(c)))]);
            }
        }
    }));
    if cx.try_global::<SettingPickers>().is_none() {
        cx.set_global(SettingPickers { states: HashMap::new(), _subs: Vec::new() });
    }
    let g = cx.global_mut::<SettingPickers>();
    g.states.insert(key, state.clone());
    g._subs.extend(subs);
    state
}

fn settings_colors(sh: &Rc<Shared>, window: &mut Window, cx: &mut App) -> AnyElement {
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    let th = cx.theme().clone();
    let theme_aware = sh.style.theme_aware;
    let rows: Vec<AnyElement> = COLOR_KEYS
        .iter()
        .map(|(key, label)| {
            let state = setting_picker(key, window, cx);
            // the value may have changed elsewhere (reset): show it
            if let Some(cur) = setting_color(key, cx) {
                let (open, value) = {
                    let s = state.read(cx);
                    (s.is_open(), s.value())
                };
                if !open && value.map(hex) != Some(hex(cur)) {
                    state.update(cx, |s, cx| s.set_value(cur, window, cx));
                }
            }
            div()
                .flex()
                .items_center()
                .justify_between()
                .py_1()
                .child(div().text_sm().child(*label))
                .child(ColorPicker::new(&state).small())
                .into_any_element()
        })
        .collect();
    let me = Rc::downgrade(sh);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Colours"))
        .when(theme_aware, |d| d.child(div().text_xs().text_color(th.muted_foreground).child("Used when “Theme-aware colors” is off.")))
        .children(rows)
        .child(
            div().pt_2().child(
                Button::new("hblock-reset").label("Reset styling to defaults").small().danger().on_click(move |_, _, cx| {
                    // keeps widths, drops the per-block colours (as the original's reset)
                    if let Some(me) = me.upgrade() {
                        me.layouts.borrow_mut().clear_colors();
                    }
                    PluginHost::set_settings(cx, ID, style_defaults());
                }),
            ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    // no `gpui_kit::*` glob here: its `test` macro would shadow the built-in one
    use super::{block_id, color_of, column_widths, finish_drag, normalise, split_sections, text_widths, width_of, Columns, Drag, Layouts, Shared, Style};
    use serde_json::{json, Map, Value};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    #[test]
    fn splits_like_the_original() {
        // `---` inside fenced code (backticks or tildes, longer closing fences) stays
        let c = split_sections("### A\nleft\n---\n```\n---\n```\n---\n~~~~\n---\n~~~~~\n---\n  ---  \nright");
        assert_eq!(c, vec!["### A\nleft", "```\n---\n```", "~~~~\n---\n~~~~~", "", "right"]);
        // a shorter or different fence doesn't close the block
        assert_eq!(split_sections("````\n```\n---\n````\n---\nb"), vec!["````\n```\n---\n````", "b"]);
        assert_eq!(split_sections("a\n----\nb"), vec!["a\n----\nb"]);
        assert_eq!(split_sections("only"), vec!["only"]);
    }

    #[test]
    fn ids_match_obsidian() {
        // measured in Obsidian: the source has no final newline and LF line ends
        assert_eq!(block_id(&normalise("left\n---\nright")), "ad74803a601b6f2d");
        assert_eq!(block_id(&normalise("win\r\n---\r\nx")), "2b9b55050700495f");
    }

    #[test]
    fn our_layouts_override_obsidians() {
        let obs = json!({
            "horizontal-block-layout-abc": {"width-0": 240.5, "width-1": 300, "bg-0": "#112233"},
            "style": {"showBorders": true},
            "previousRelease": "1.1.1"
        });
        let ours = json!({"horizontal-block-layout-abc": {"width-1": null, "fg-1": "#ffffff"}});
        let mut l = Layouts::from_parts(obs.as_object().unwrap().clone(), ours.as_object().unwrap().clone(), None);
        let g = l.get("abc");
        assert_eq!(width_of(&g, 0), Some(240.5));
        assert_eq!(width_of(&g, 1), None, "removed by us");
        assert!(color_of(&g, "bg", 0).is_some() && color_of(&g, "fg", 1).is_some());
        assert!(l.merged.get("style").is_none(), "only layouts are read from Obsidian's file");
        l.clear_colors();
        let g = l.get("abc");
        assert!(color_of(&g, "bg", 0).is_none() && color_of(&g, "fg", 1).is_none());
        assert_eq!(width_of(&g, 0), Some(240.5), "reset keeps widths");
    }

    #[test]
    fn dragging_fixes_the_left_column_and_frees_the_right() {
        let layout = json!({"width-0": 100, "width-1": 150, "width-2": 90}).as_object().unwrap().clone();
        assert_eq!(column_widths(&layout, 3, None), vec![Some(100.0), Some(150.0), Some(90.0)]);
        let d = Drag { id: "x".into(), index: 0, start_x: 0.0, start_w: 100.0, width: 180.0, max_w: 500.0 };
        assert_eq!(column_widths(&layout, 3, Some(&d)), vec![Some(180.0), None, Some(90.0)]);
        let sh = shared(Map::new());
        *sh.drag.borrow_mut() = Some(Drag { id: "x".into(), index: 1, start_x: 0.0, start_w: 100.0, width: 222.333, max_w: 500.0 });
        finish_drag(&sh);
        let g = sh.layouts.borrow().get("x");
        assert_eq!(width_of(&g, 1), Some(222.33));
        assert_eq!(width_of(&g, 2), None);
        assert!(sh.drag.borrow().is_none());
    }

    #[test]
    fn column_text_widths_add_up_to_the_block() {
        let s = Style::from_settings(&Map::new()); // divider 2, gap 0, padding 12, no borders
        // 3 flexible columns in 700 px: 2 dividers, 3 × 4 px margins
        let w = text_widths(700.0, &[None, None, None], &s);
        let col = (700.0 - 4.0 - 12.0) / 3.0;
        assert!(w.iter().all(|x| (x - (col - 24.0)).abs() < 0.01), "{w:?}");
        // a fixed column keeps its width, the others share the rest; a gap counts between all items
        let s = Style::from_settings(json!({"blockGap": 8, "showBorders": true}).as_object().unwrap());
        let w = text_widths(700.0, &[Some(300.0), None], &s);
        assert_eq!(w[0], 300.0 - 24.0 - 4.0);
        assert_eq!(w[1], 700.0 - 300.0 - 2.0 - 16.0 - 8.0 - 24.0 - 4.0);
        // never narrower than an image can usefully be
        assert_eq!(text_widths(100.0, &[Some(90.0), None], &s)[1], 32.0);
    }

    #[test]
    fn style_falls_back_to_the_originals_defaults() {
        let s = Style::from_settings(&Map::new());
        assert!(s.theme_aware && !s.show_borders && !s.show_toolbar);
        assert_eq!((s.divider_thickness, s.block_padding, s.block_gap, s.divider_style.as_str()), (2.0, 12.0, 0.0, "solid"));
        let s = Style::from_settings(json!({"dividerThickness": 9, "dividerOpacity": -1}).as_object().unwrap());
        assert_eq!((s.divider_thickness, s.divider_opacity), (5.0, 0.0), "clamped as the original does");
    }

    fn shared(settings: Map<String, Value>) -> Shared {
        Shared {
            style: Style::from_settings(&settings),
            layouts: RefCell::new(Layouts::from_parts(Map::new(), Map::new(), None)),
            drag: RefCell::new(None),
            col_w: RefCell::default(),
            pickers: RefCell::default(),
        }
    }

    /// The export compiles with the PDF prelude for every look the settings allow.
    #[test]
    fn export_compiles_with_every_style() {
        use crate::export::compile::{run, Job};
        use crate::plugins::pdf_export::{prelude, Page};
        let src = "First *column*\n---\n- a list\n- item\n---\n```\n---\n```";
        for (style, extra) in [
            ("solid", json!({})),
            ("dashed", json!({"showBorders": true, "alternatingShading": true, "dividerOpacity": 0.5})),
            ("dotted", json!({"themeAware": false, "transparentBackground": true})),
            ("transparent", json!({"themeAware": false, "blockGap": 8})),
        ] {
            let mut settings = extra.as_object().unwrap().clone();
            settings.insert("dividerStyle".into(), json!(style));
            let sh = shared(settings);
            let id = block_id(src);
            sh.layouts.borrow_mut().set(&id, "width-0", json!(180.5));
            sh.layouts.borrow_mut().set(&id, "bg-1", json!("#336699"));
            sh.layouts.borrow_mut().set(&id, "title-1", json!("A \"title\" #with [markup]"));
            let body = Columns(Rc::new(sh)).typst(src, &|m: &str| crate::export::typst::lit(m));
            let source = format!("{}\n{body}\n", prelude("t", &Page::simple(10.0), &Default::default()));
            let fonts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            let out = run(Job { source, math: vec![], files: vec![], sans: fonts(&["IBM Plex Sans", "Noto Sans", "DejaVu Sans"]), mono: fonts(&["DejaVu Sans Mono"]) });
            match out {
                Ok(o) => assert!(o.warnings.is_empty(), "{style}: {:?}\n{body}", o.warnings),
                Err(e) => panic!("{style}: {e}\n{body}"),
            }
        }
    }

    #[test]
    fn exports_a_typst_grid() {
        let src = "a\n---\nb\n---\nc";
        let sh = shared(Map::new());
        sh.layouts.borrow_mut().set(&block_id(src), "width-0", json!(240));
        sh.layouts.borrow_mut().set(&block_id(src), "fg-2", json!("#ff0000"));
        let t = Columns(Rc::new(sh)).typst(src, &|m: &str| format!("<{m}>"));
        assert!(t.contains("columns: (240 * u_, 2.0 * u_, 1fr, 2.0 * u_, 1fr)"), "{t}");
        assert!(t.contains("<a>") && t.contains("<c>") && t.contains("fill: rgb(255, 0, 0)"), "{t}");
        assert!(t.contains("(left: 2.0 * u_ + c_.accent)"), "solid divider in the accent colour: {t}");
        // the spike's Typst compiler checks this output (see docs/milestones/M2-feedback.md)
        if let Ok(dir) = std::env::var("TEPHRA_TYPST_OUT") {
            let body = Columns(Rc::new(shared(Map::new()))).typst("=== Left\nSome *bold* text.\n---\n=== Right\n- a list", &|m: &str| m.to_string());
            let pre = "#let u_ = 0.75pt\n#let c_ = (text: rgb(34, 34, 34), accent: rgb(138, 92, 245))\n";
            std::fs::write(Path::new(&dir).join("main.typ"), format!("#set page(paper: \"a4\", margin: 10mm)\n{pre}= Layout\n\n{body}\n\nAfter the block.\n")).unwrap();
        }
    }
}
