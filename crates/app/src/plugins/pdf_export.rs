//! PDF export (replaces "Better Export PDF"): typesets a note with Typst (see `export/`) and
//! shows a preview of its pages before you save. With your settings (read from the plugin's
//! `data.json`, never written): A4, 10 mm margins, the note title as heading and in the page
//! header, `page / total` in the footer. Headings become the PDF's bookmarks; an
//! ```` ```academic-toc ```` block becomes a table of contents; formulas are vector (MathJax).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;
use super::api::{Action, MenuItem};
use super::PluginHost;
use crate::export::compile::{self, Job, Output};
use crate::export::typst::{lit, on_white, str_arg, Ctx, Emitter, Look, Rgb, Style};

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub paper: String,
    pub landscape: bool,
    /// top, right, bottom, left.
    pub margins_mm: [f64; 4],
    pub title: bool,
    pub header: bool,
    pub footer: bool,
    pub font_pt: f64,
    /// Downscale, 0.5..=1.0: text (and with it formulas), images and charts.
    pub scale: f64,
}

impl Page {
    pub fn simple(margin_mm: f64) -> Self {
        Self { paper: "A4".into(), landscape: false, margins_mm: [margin_mm; 4], title: true, header: true, footer: true, font_pt: 11.0, scale: 1.0 }
    }
}

/// The document settings before the body: page, fonts, the reading view's look (`look`) and
/// the helpers that draw it (`export::typst::HELPERS`).
pub fn prelude(title: &str, p: &Page, look: &Look) -> String {
    let t = lit(title);
    let header = if p.header { format!("align(center, text(size: 8pt, fill: luma(110))[{t}])") } else { "none".into() };
    let footer = if p.footer { "context align(center, text(size: 8pt, fill: luma(110))[#counter(page).display() / #counter(page).final().first()])".to_string() } else { "none".into() };
    let [mt, mr, mb, ml] = p.margins_mm;
    let size = p.font_pt * p.scale;
    // `u_`: one pixel of the reading view, whose text is 16 px
    format!(
        "#set document(title: {})\n#set page(paper: {}, flipped: {}, margin: (top: {mt:.1}mm, right: {mr:.1}mm, bottom: {mb:.1}mm, left: {ml:.1}mm), header: {header}, footer: {footer})\n#let u_ = {:.4}pt\n#let body_ = {size:.2}pt\n#let asc_ = @@ASC@@\n#let desc_ = @@DESC@@\n#set text(font: @@SANS@@, size: {size:.2}pt, hyphenate: false)\n{}{}\n",
        str_arg(title),
        str_arg(&p.paper.to_lowercase()),
        p.landscape,
        size / 16.0,
        look.defs(),
        crate::export::typst::HELPERS,
    )
}

/// What the dialog shows: still typesetting, the result, or an error.
enum State {
    Working,
    Done(Box<Output>, Vec<Arc<Image>>),
    Failed(String),
}

fn hsla_rgb(c: Hsla) -> (u8, u8, u8) {
    let r: gpui_kit::Rgba = c.into();
    ((r.r * 255.0) as u8, (r.g * 255.0) as u8, (r.b * 255.0) as u8)
}

/// A colour as it shows on white paper (translucent ones mixed in).
fn on_paper(c: Hsla) -> Rgb {
    let a = gpui_kit::Rgba::from(c).a;
    on_white(hsla_rgb(c), a)
}

/// The reading view's look for a note with `classes`, in the theme's light colours.
pub fn look_for(classes: &[String], cx: &App) -> Look {
    let mut l = Look { center_math: crate::app_settings::AppSettings::flag(cx, "centerDisplayMath"), ..Look::default() };
    let Some(t) = cx.try_global::<crate::theme::ThemeCss>() else { return l };
    let c = |n: &str| t.color(n, false, classes);
    let set = |slot: &mut Rgb, n: &str| {
        if let Some(v) = c(n) {
            *slot = on_paper(v);
        }
    };
    set(&mut l.text, "--text-normal");
    set(&mut l.muted, "--text-muted");
    set(&mut l.accent, "--interactive-accent");
    set(&mut l.on_accent, "--text-on-accent");
    set(&mut l.link, "--link-color");
    set(&mut l.border, "--table-border-color");
    set(&mut l.code_bg, "--code-background");
    // gpui-kit draws inline code on the theme's "accent" (Obsidian's hover background)
    set(&mut l.inline_code_bg, "--background-modifier-hover");
    set(&mut l.highlight, "--text-highlight-bg");
    set(&mut l.quote, "--blockquote-border-color");
    set(&mut l.hr, "--hr-color");
    l.table_head = c("--table-header-background").filter(|v| gpui_kit::Rgba::from(*v).a > 0.01).map(on_paper);
    if let Some(w) = t.length("--table-border-width", false, classes, 16.0) {
        l.border_px = w.max(0.0);
    }
    if let Some(w) = t.var("--table-header-weight", false, classes).and_then(|w| w.trim().parse::<u16>().ok()) {
        l.table_head_weight = w;
    }
    if let Some(w) = t.length("--hr-thickness", false, classes, 16.0) {
        l.hr_px = w;
    }
    for (i, h) in l.headings.iter_mut().enumerate() {
        let n = i + 1;
        if let Some(px) = t.length(&format!("--h{n}-size"), false, classes, 16.0) {
            h.0 = px / 16.0;
        }
        if let Some(w) = t.var(&format!("--h{n}-weight"), false, classes).and_then(|w| w.trim().parse::<u16>().ok()) {
            h.1 = w;
        }
        // `inherit` (no colour): the text's; the editor mutes H6
        h.2 = c(&format!("--h{n}-color")).map(on_paper).or((n == 6).then_some(l.muted));
    }
    l
}

/// An icon's SVG (Lucide, drawn with `currentColor`).
fn icon_svg(icon: gpui_kit::assets::IconName) -> Option<String> {
    use gpui_kit::AssetSource as _;
    let b = crate::icons::AppAssets.load(&icon.path()).ok()??;
    String::from_utf8(b.into_owned()).ok()
}

/// The fonts the editor draws with (its reading view), then fallbacks: (text, code).
fn editor_fonts(cx: &App) -> (Vec<String>, Vec<String>) {
    let theme = cx.theme();
    // GPUI's ".SystemUIFont" is IBM Plex Sans on Linux
    let sans = match theme.font_family.as_ref() {
        ".SystemUIFont" | ".ZedSans" => "IBM Plex Sans".to_string(),
        f => f.to_string(),
    };
    let mono = theme.mono_font_family.to_string();
    (
        vec![sans, "IBM Plex Sans".into(), "Noto Sans".into(), "DejaVu Sans".into()],
        vec![mono, "Lilex".into(), "IBM Plex Mono".into(), "JetBrains Mono".into(), "DejaVu Sans Mono".into()],
    )
}

/// Builds the Typst document for a note (on the UI thread: it asks the plugins).
pub fn build(rel: &str, text: &str, page: &Page, cx: &App) -> Option<Job> {
    let ws = WorkspaceHandle::current(cx)?;
    let vault = ws.vault(cx)?;
    let v = vault.borrow();
    let abs = v.root.join(rel);
    let host = cx.try_global::<PluginHost>()?;
    // plugin numbers for this note (callouts, equations)
    let note = NoteCtx { path: rel, text, index: &v.index };
    let mut numbers: HashMap<usize, String> = HashMap::new();
    for (_, r) in &host.reg.callout_renderers {
        for (k, l) in r.numbers(&note) {
            numbers.entry(k).or_insert(l);
        }
    }
    let labelers: Vec<_> = host.reg.link_labelers.iter().map(|(_, l)| l.clone()).collect();
    let link_label = |target: &str| -> Option<String> {
        let (n, sub) = target.split_once('#')?;
        let t = if n.is_empty() { Some(rel.to_string()) } else { v.index.resolve(n, rel) }?;
        let tt = if t == rel { text } else { v.index.text(&t)? };
        let ctx = NoteCtx { path: &t, text: tt, index: &v.index };
        labelers.iter().find_map(|l| l.label(&ctx, sub))
    };
    let style = |kind: &str, meta: &str| {
        let s = crate::editor::render::callout_style_for(kind, meta, false, cx); // paper is white
        Style { look: s.look, color: hsla_rgb(s.color), label: s.label, icon: s.icon.and_then(icon_svg) }
    };
    let code = |lang: &str, code: &str, md: &dyn Fn(&str) -> String| host.reg.code_block_renderer(lang).and_then(|r| r.export_typst(code, md));
    let code_note = |lang: &str| match host.code_mode(lang) {
        crate::editor::projection::CodeMode::Note(n) => Some(n),
        _ => None,
    };
    let callout_body = |kind: &str, meta: &str, body: &[String], md: &dyn Fn(&str) -> String| host.reg.callout_renderer(kind, meta).and_then(|r| r.export_body(kind, meta, body, md));
    let body_end = |kind: &str| host.reg.callout_renderer(kind, "").and_then(|r| r.body_end(kind));
    let embed = |name: &str| v.resolve(name, &abs);
    let classes = {
        let mut cut = text.len().min(8192);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        tephra_core::index::cssclasses(&text[..cut])
    };
    let look = look_for(&classes, cx);
    let e = Emitter::new(Ctx { numbers: &numbers, link_label: &link_label, style: &style, code: &code, code_note: &code_note, callout_body: &callout_body, body_end: &body_end, embed: &embed });
    let body = e.blocks(text, 0);
    let title = Path::new(rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    // the title as a heading, unless the note starts with that H1 already
    let own_h1 = tephra_core::markdown::parse_blocks(text)
        .into_iter()
        .find(|b| matches!(b.kind, tephra_core::markdown::BlockKind::Heading { .. }))
        .is_some_and(|b| b.kind == (tephra_core::markdown::BlockKind::Heading { level: 1 }) && text[b.content.clone()].trim() == title);
    // the title as Obsidian's inline title: an H1, half a line above the note
    let heading = if page.title && !own_h1 { format!("#heading(level: 1)[{}]\n\n#v(0.5em)\n\n", lit(&title)) } else { String::new() };
    let source = format!("{}\n{heading}{body}\n", prelude(&title, page, &look));
    let (sans, mono) = editor_fonts(cx);
    Some(Job { source, math: e.math.into_inner(), files: e.files.into_inner(), sans, mono })
}

/// Margins as in Obsidian's export dialog, plus your own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Margin {
    /// The plugin setting (10 mm, from Better Export PDF).
    Default,
    None,
    Small,
    Custom,
}

impl Margin {
    fn key(self) -> &'static str {
        match self {
            Margin::Default => "default",
            Margin::None => "none",
            Margin::Small => "small",
            Margin::Custom => "custom",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Margin::Default => "Default",
            Margin::None => "None",
            Margin::Small => "Small",
            Margin::Custom => "Custom",
        }
    }
    fn of(s: &str) -> Self {
        match s {
            "none" => Margin::None,
            "small" => Margin::Small,
            "custom" => Margin::Custom,
            _ => Margin::Default,
        }
    }
}

/// Downscale range (%): below ~50% text gets too small to read; above 100% it isn't a downscale.
pub const SCALE_MIN: f64 = 50.0;
pub const SCALE_MAX: f64 = 100.0;
const PX_MM: f64 = 25.4 / 96.0;

/// The export dialog's choices (remembered when you save).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub header: bool,
    pub footer: bool,
    pub landscape: bool,
    pub title: bool,
    pub margin: Margin,
    /// Custom margins in px (1 px = 1/96 in): top/bottom and left/right.
    pub tb_px: f64,
    pub lr_px: f64,
    pub scale_pct: f64,
}

impl Options {
    pub fn page(&self, base: &Page) -> Page {
        let m = match self.margin {
            Margin::Default => base.margins_mm,
            Margin::None => [0.0; 4],
            Margin::Small => [5.0; 4],
            Margin::Custom => {
                let (v, h) = (self.tb_px.clamp(0.0, 300.0) * PX_MM, self.lr_px.clamp(0.0, 300.0) * PX_MM);
                [v, h, v, h]
            }
        };
        Page { landscape: self.landscape, margins_mm: m, title: self.title, header: self.header, footer: self.footer, scale: self.scale_pct.clamp(SCALE_MIN, SCALE_MAX) / 100.0, ..base.clone() }
    }
}

fn settings_of(settings: &Map<String, Value>) -> (Page, Options) {
    let n = |k: &str, d: f64| settings.get(k).and_then(Value::as_f64).unwrap_or(d);
    let b = |k: &str, d: bool| settings.get(k).and_then(Value::as_bool).unwrap_or(d);
    let mut base = Page::simple(n("marginMm", 10.0));
    base.paper = settings.get("pageSize").and_then(Value::as_str).unwrap_or("A4").to_string();
    base.font_pt = n("fontSize", 11.0);
    let opts = Options {
        header: b("displayHeader", true),
        footer: b("displayFooter", true),
        landscape: b("landscape", false),
        title: b("showTitle", true),
        margin: Margin::of(settings.get("marginType").and_then(Value::as_str).unwrap_or("default")),
        tb_px: n("marginTopBottomPx", 38.0),
        lr_px: n("marginLeftRightPx", 38.0),
        scale_pct: n("scalePercent", 100.0),
    };
    (base, opts)
}

/// The note's current text (unsaved edits included, if it's open), else the file's.
fn note_text(rel: &str, cx: &App) -> Option<String> {
    let ws = WorkspaceHandle::current(cx)?;
    let vault = ws.vault(cx)?;
    let open = ws.0.upgrade().and_then(|w| w.read(cx).all_editors().into_iter().find(|e| vault.borrow().relative(&e.read(cx).path) == rel));
    match open {
        Some(e) => Some(e.read(cx).doc.text()),
        None => std::fs::read_to_string(vault.borrow().root.join(rel)).ok(),
    }
}

/// The export dialog: options on the left, the pages on the right; changing an option
/// typesets again (after a short pause, so a slider drag doesn't start ten jobs).
pub struct ExportView {
    rel: String,
    title: String,
    base: Page,
    opts: Options,
    state: State,
    /// Bumped per change; results of older jobs are dropped.
    generation: u64,
    slider: Entity<SliderState>,
    tb: Entity<InputState>,
    lr: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl ExportView {
    fn new(rel: String, base: Page, opts: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let slider = cx.new(|_| SliderState::new().min(SCALE_MIN as f32).max(SCALE_MAX as f32).step(5.0).default_value(opts.scale_pct as f32));
        let tb = cx.new(|cx| InputState::new(window, cx).default_value(format!("{}", opts.tb_px)));
        let lr = cx.new(|cx| InputState::new(window, cx).default_value(format!("{}", opts.lr_px)));
        let subs = vec![
            cx.subscribe(&slider, |this, _, ev: &SliderEvent, cx| {
                // the label follows the drag; typesetting waits for the release
                match ev {
                    SliderEvent::Change(v) => {
                        this.opts.scale_pct = v.start() as f64;
                        cx.notify();
                    }
                    SliderEvent::Release(v) => {
                        this.opts.scale_pct = v.start() as f64;
                        this.changed(cx);
                    }
                }
            }),
            cx.subscribe(&tb, |this, s, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    if let Ok(v) = s.read(cx).value().trim().parse::<f64>() {
                        this.opts.tb_px = v;
                        this.changed(cx);
                    }
                }
            }),
            cx.subscribe(&lr, |this, s, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    if let Ok(v) = s.read(cx).value().trim().parse::<f64>() {
                        this.opts.lr_px = v;
                        this.changed(cx);
                    }
                }
            }),
        ];
        let title = Path::new(&rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut this = Self { rel, title, base, opts, state: State::Working, generation: 0, slider, tb, lr, _subs: subs };
        this.typeset(cx);
        this
    }

    /// An option changed: typeset again after a short pause.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let g = self.generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = this.update(cx, |v, cx| {
                if v.generation == g {
                    v.typeset(cx);
                }
            });
        })
        .detach();
    }

    fn typeset(&mut self, cx: &mut Context<Self>) {
        let page = self.opts.page(&self.base);
        let job = note_text(&self.rel, cx).and_then(|t| build(&self.rel, &t, &page, cx));
        let Some(job) = job else {
            self.state = State::Failed("the note could not be read".into());
            return;
        };
        if let Ok(dir) = std::env::var("TEPHRA_TYPST_OUT") {
            let _ = std::fs::write(Path::new(&dir).join("main.typ"), &job.source);
        }
        self.state = State::Working;
        let g = self.generation;
        let task = cx.background_spawn(async move { compile::run_isolated(job) });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            let _ = this.update(cx, |v, cx| {
                if v.generation != g {
                    return; // an option changed meanwhile: a newer job is on its way
                }
                let script = std::env::var_os("TEPHRA_SCRIPT").is_some();
                v.state = match res {
                    Ok(out) => {
                        if script {
                            println!("PDF pages={} bytes={} math_ms={:.0} compile_ms={:.0} pdf_ms={:.0} warnings={:?}", out.pages.len(), out.pdf.len(), out.ms.0, out.ms.1, out.ms.2, out.warnings);
                            if let Ok(p) = std::env::var("TEPHRA_PDF_OUT") {
                                let _ = std::fs::write(p, &out.pdf);
                            }
                        }
                        let imgs = out.pages.iter().map(|p| Arc::new(Image::from_bytes(ImageFormat::Png, p.clone()))).collect();
                        State::Done(Box::new(out), imgs)
                    }
                    Err(e) => {
                        if script {
                            println!("PDF error {e}");
                        }
                        State::Failed(e)
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Remembers the dialog's choices (next export starts from them).
    fn remember(&self, cx: &mut App) {
        let o = &self.opts;
        PluginHost::set_settings(
            cx,
            "pdf-export",
            vec![
                ("displayHeader", Value::Bool(o.header)),
                ("displayFooter", Value::Bool(o.footer)),
                ("landscape", Value::Bool(o.landscape)),
                ("showTitle", Value::Bool(o.title)),
                ("marginType", Value::from(o.margin.key())),
                ("marginTopBottomPx", Value::from(o.tb_px)),
                ("marginLeftRightPx", Value::from(o.lr_px)),
                ("scalePercent", Value::from(o.scale_pct)),
            ],
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Done(out, _) = &self.state else { return };
        let bytes = out.pdf.clone();
        self.remember(cx);
        let dir = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).map(|h| if h.join("Documents").is_dir() { h.join("Documents") } else { h }).unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{}.pdf", self.title)));
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(path))) = rx.await {
                    let res = std::fs::write(&path, &bytes);
                    let _ = cx.update(|window, cx| {
                        window.close_dialog(cx);
                        let msg = match res {
                            Ok(()) => format!("Saved {}", path.display()),
                            Err(e) => format!("Could not save: {e}"),
                        };
                        window.push_notification(msg, cx);
                    });
                }
            })
            .detach();
    }
}

impl Render for ExportView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toggle = |id: &'static str, label: &'static str, on: bool, set: fn(&mut Options, bool), cx: &mut Context<Self>| {
            Switch::new(id).checked(on).label(label).on_click(cx.listener(move |v, on: &bool, _, cx| {
                set(&mut v.opts, *on);
                v.changed(cx);
            }))
        };
        let o = self.opts.clone();
        let margin_btn = Button::new("pdf-margin").outline().small().label(format!("Margins: {}", o.margin.label())).dropdown_menu({
            let this = cx.entity().downgrade();
            move |mut menu, _, _| {
                for m in [Margin::Default, Margin::None, Margin::Small, Margin::Custom] {
                    let this = this.clone();
                    menu = menu.item(PopupMenuItem::new(m.label()).checked(o.margin == m).on_click(move |_, _, cx| {
                        let _ = this.update(cx, |v, cx| {
                            v.opts.margin = m;
                            v.changed(cx);
                        });
                    }));
                }
                menu
            }
        });
        let options = v_flex()
            .w(px(230.))
            .flex_none()
            .gap_3()
            .text_sm()
            .child(toggle("pdf-title", "Filename as title", o.title, |o, v| o.title = v, cx))
            .child(toggle("pdf-header", "Header (note name)", o.header, |o, v| o.header = v, cx))
            .child(toggle("pdf-footer", "Footer (page / total)", o.footer, |o, v| o.footer = v, cx))
            .child(toggle("pdf-landscape", "Landscape", o.landscape, |o, v| o.landscape = v, cx))
            .child(margin_btn)
            .when(o.margin == Margin::Custom, |d| {
                d.child(v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("Top / bottom (px)")).child(Input::new(&self.tb).small()))
                    .child(v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("Left / right (px)")).child(Input::new(&self.lr).small()))
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("Downscale: {:.0}%", o.scale_pct)))
                    .child(Slider::new(&self.slider)),
            );
        let (body, info): (AnyElement, String) = match &self.state {
            State::Working => (div().p_8().text_color(theme.muted_foreground).child("Typesetting…").into_any_element(), String::new()),
            State::Failed(e) => (div().p_4().text_color(theme.danger).child(format!("Could not typeset the note: {e}")).into_any_element(), String::new()),
            State::Done(out, imgs) => {
                // only the pages in view are drawn (decoded): a long note stays cheap
                let imgs = imgs.clone();
                let page = self.opts.page(&self.base);
                let (pw, ph) = if page.landscape { (297.0, 210.0) } else { (210.0, 297.0) };
                let w = 520.0f32;
                let h = w * ph / pw;
                let list = uniform_list("pdf-pages", imgs.len(), move |range, _, _| {
                    range.map(|i| div().w_full().h(px(h + 12.)).flex().justify_center().child(div().w(px(w)).h(px(h)).bg(gpui_kit::white()).shadow_md().child(img(imgs[i].clone()).size_full()))).collect::<Vec<_>>()
                });
                (list.size_full().into_any_element(), format!("{} page{} · {} KB", out.pages.len(), if out.pages.len() == 1 { "" } else { "s" }, out.pdf.len() / 1024))
            }
        };
        v_flex()
            .gap_2()
            .child(div().text_xs().text_color(theme.muted_foreground).child(if info.is_empty() { " ".to_string() } else { info }))
            .child(h_flex().items_start().gap_4().child(options).child(div().flex_1().h(px(560.)).bg(theme.muted).pt_3().child(body)))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("pdf-cancel").outline().label("Close").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("pdf-save").primary().label("Save PDF…").disabled(!matches!(self.state, State::Done(..))).on_click(cx.listener(|v, _, w, cx| v.save(w, cx)))),
            )
    }
}

/// Opens the export dialog for `rel` (vault path).
fn export(rel: String, base: Page, opts: Options, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ExportView::new(rel, base, opts, window, cx));
    window.open_dialog(cx, move |dialog, _, _| dialog.title("Export to PDF").w(px(900.)).child(view.clone()));
}

pub struct PdfExport {
    manifest: PluginManifest,
}

impl PdfExport {
    /// Defaults from Better Export PDF's settings (read only).
    pub fn new(vault: &Path) -> Self {
        let saved: Map<String, Value> = std::fs::read_to_string(vault.join(".obsidian/plugins/better-export-pdf/data.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let prev = saved.get("prevConfig").cloned().unwrap_or(Value::Null);
        let b = |k: &str| Value::Bool(saved.get(k).and_then(Value::as_bool).unwrap_or(true));
        let paper = prev["pageSize"].as_str().unwrap_or("A4").to_string();
        let margin = prev["marginTop"].as_str().and_then(|m| m.parse::<f64>().ok()).or_else(|| prev["marginTop"].as_f64()).unwrap_or(10.0);
        let papers = vec![("A4", "A4"), ("A3", "A3"), ("A5", "A5"), ("Letter", "Letter"), ("Legal", "Legal")];
        Self {
            manifest: PluginManifest {
                id: "pdf-export",
                name: "PDF export",
                version: "0.1.0",
                description: "Export a note to PDF (Typst): page header and numbers, bookmarks from headings, vector math, a preview before saving.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "pageSize", label: "Page size", description: "", kind: SettingKind::Choice(papers), default: Value::from(paper) },
                    SettingSpec { key: "marginMm", label: "Default margins (mm)", description: "The \"Default\" margins of the export dialog, all four sides.", kind: SettingKind::Number, default: Value::from(margin) },
                    SettingSpec { key: "fontSize", label: "Font size (pt)", description: "Before downscaling.", kind: SettingKind::Number, default: Value::from(11.0) },
                    // the export dialog's choices, remembered
                    SettingSpec { key: "showTitle", label: "", description: "", kind: SettingKind::Hidden, default: b("showTitle") },
                    SettingSpec { key: "displayHeader", label: "", description: "", kind: SettingKind::Hidden, default: b("displayHeader") },
                    SettingSpec { key: "displayFooter", label: "", description: "", kind: SettingKind::Hidden, default: b("displayFooter") },
                    SettingSpec { key: "landscape", label: "", description: "", kind: SettingKind::Hidden, default: Value::Bool(prev["landscape"].as_bool().unwrap_or(false)) },
                    SettingSpec { key: "marginType", label: "", description: "", kind: SettingKind::Hidden, default: Value::from("default") },
                    SettingSpec { key: "marginTopBottomPx", label: "", description: "", kind: SettingKind::Hidden, default: Value::from(38.0) },
                    SettingSpec { key: "marginLeftRightPx", label: "", description: "", kind: SettingKind::Hidden, default: Value::from(38.0) },
                    SettingSpec { key: "scalePercent", label: "", description: "", kind: SettingKind::Hidden, default: Value::from(prev["scale"].as_f64().unwrap_or(100.0).clamp(SCALE_MIN, SCALE_MAX)) },
                ],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for PdfExport {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let (base, opts) = settings_of(settings);
        let (b1, o1) = (base.clone(), opts.clone());
        let run: Action = Rc::new(move |a: &mut ActionCtx| {
            if let Some(rel) = a.workspace().and_then(|w| w.active_note(a.cx)) {
                export(rel, b1.clone(), o1.clone(), a.window, a.cx);
            }
        });
        reg.command(Command { id: "export".into(), name: "Export current note to PDF".into(), hotkey: None, run: run.clone() });
        reg.ribbon(RibbonItem { id: "export".into(), icon: gpui_kit::assets::IconName::FileDown, tooltip: "Export to PDF".into(), run });
        reg.menu_item(MenuItem {
            id: "export".into(),
            label: "Export to PDF…".into(),
            target: MenuTarget::File,
            run: Rc::new(move |a: &mut ActionCtx, rel: Option<&str>| {
                if let Some(rel) = rel.filter(|r| r.to_lowercase().ends_with(".md")) {
                    export(rel.to_string(), base.clone(), opts.clone(), a.window, a.cx);
                }
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{prelude, Look, Margin, Options, Page};

    #[test]
    fn prelude_has_page_header_footer() {
        let t = prelude("Note \"x\"", &Page::simple(10.0), &Look::default());
        assert!(t.contains("paper: \"a4\", flipped: false, margin: (top: 10.0mm, right: 10.0mm, bottom: 10.0mm, left: 10.0mm)"), "{t}");
        assert!(t.contains("#counter(page).display() / #counter(page).final().first()"));
        assert!(t.contains("#set document(title: \"Note \\\"x\\\"\")"));
    }

    #[test]
    fn options_make_the_page() {
        let base = Page::simple(10.0);
        let mut o = Options { header: false, footer: true, landscape: true, title: false, margin: Margin::Custom, tb_px: 96.0, lr_px: 48.0, scale_pct: 80.0 };
        let p = o.page(&base);
        assert!(p.landscape && !p.header && p.footer && !p.title);
        assert!((p.margins_mm[0] - 25.4).abs() < 0.01 && (p.margins_mm[1] - 12.7).abs() < 0.01, "px to mm");
        assert!((p.scale - 0.8).abs() < 1e-9);
        let t = prelude("x", &p, &Look::default());
        assert!(t.contains("flipped: true") && t.contains("header: none") && t.contains("#let u_ = 0.5500pt") && t.contains("size: 8.80pt"), "{t}");
        o.scale_pct = 10.0;
        assert!((o.page(&base).scale - 0.5).abs() < 1e-9, "clamped to the safe minimum");
        o.margin = Margin::None;
        assert_eq!(o.page(&base).margins_mm, [0.0; 4]);
        o.margin = Margin::Default;
        assert_eq!(o.page(&base).margins_mm, [10.0; 4]);
    }
}
