//! PDF export (replaces "Better Export PDF"): typesets a note with Typst (see `export/`) and
//! shows a preview of its pages before you save. With your settings (read from the plugin's
//! `data.json`, never written): A4, 10 mm margins, the note title as heading and in the page
//! header, `page / total` in the footer. Headings become the PDF's bookmarks; an
//! ```` ```academic-toc ```` block becomes a table of contents; formulas are vector (MathJax).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, WindowExt as _};
use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;
use super::api::{Action, MenuItem};
use super::PluginHost;
use crate::export::compile::{self, Job, Output};
use crate::export::typst::{lit, str_arg, Ctx, Emitter, Style};

#[derive(Clone, Debug)]
pub struct Page {
    pub paper: String,
    pub margin_mm: f64,
    pub title: bool,
    pub header: bool,
    pub footer: bool,
    pub font_pt: f64,
}

/// The document settings before the body.
pub fn prelude(title: &str, p: &Page) -> String {
    let t = lit(title);
    let header = if p.header { format!("align(center, text(size: 8pt, fill: luma(110))[{t}])") } else { "none".into() };
    let footer = if p.footer { "context align(center, text(size: 8pt, fill: luma(110))[#counter(page).display() / #counter(page).final().first()])".to_string() } else { "none".into() };
    format!(
        "#set document(title: {})\n#set page(paper: {}, margin: {}mm, header: {header}, footer: {footer})\n#set text(font: @@SANS@@, size: {}pt, hyphenate: false)\n#set par(leading: 0.65em, spacing: 1.1em)\n#show heading: set block(above: 1.3em, below: 0.7em)\n#show raw: set text(font: @@MONO@@, size: 0.9em)\n",
        str_arg(title),
        str_arg(&p.paper.to_lowercase()),
        p.margin_mm,
        p.font_pt
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
        let s = crate::editor::render::callout_style(kind, meta, cx);
        Style { look: s.look, color: hsla_rgb(s.color), label: s.label }
    };
    let code = |lang: &str, code: &str, md: &dyn Fn(&str) -> String| host.reg.code_block_renderer(lang).and_then(|r| r.export_typst(code, md));
    let callout_body = |kind: &str, meta: &str, body: &[String], md: &dyn Fn(&str) -> String| host.reg.callout_renderer(kind, meta).and_then(|r| r.export_body(kind, meta, body, md));
    let body_end = |kind: &str| host.reg.callout_renderer(kind, "").and_then(|r| r.body_end(kind));
    let embed = |name: &str| v.resolve(name, &abs);
    let no_border = obsidilight_core::index::parse_note(rel, text).frontmatter.is_some_and(|f| f.contains("NoBorder"));
    let e = Emitter::new(Ctx { numbers: &numbers, link_label: &link_label, style: &style, code: &code, callout_body: &callout_body, body_end: &body_end, embed: &embed, no_border });
    let body = e.blocks(text, 0);
    let title = Path::new(rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    // the title as a heading, unless the note starts with that H1 already
    let own_h1 = obsidilight_core::markdown::parse_blocks(text)
        .into_iter()
        .find(|b| matches!(b.kind, obsidilight_core::markdown::BlockKind::Heading { .. }))
        .is_some_and(|b| b.kind == (obsidilight_core::markdown::BlockKind::Heading { level: 1 }) && text[b.content.clone()].trim() == title);
    let heading = if page.title && !own_h1 { format!("= {}\n\n", lit(&title)) } else { String::new() };
    let source = format!("{}\n{heading}{body}\n", prelude(&title, page));
    Some(Job { source, math: e.math.into_inner(), files: e.files.into_inner() })
}

fn page_of(settings: &Map<String, Value>) -> Page {
    let n = |k: &str, d: f64| settings.get(k).and_then(Value::as_f64).unwrap_or(d);
    let b = |k: &str| settings.get(k).and_then(Value::as_bool).unwrap_or(true);
    Page { paper: settings.get("pageSize").and_then(Value::as_str).unwrap_or("A4").to_string(), margin_mm: n("marginMm", 10.0), title: b("showTitle"), header: b("displayHeader"), footer: b("displayFooter"), font_pt: n("fontSize", 11.0) }
}

/// Exports `rel` (vault path): typesets in the background, then shows the preview.
fn export(rel: String, page: Page, window: &mut Window, cx: &mut App) {
    let Some(ws) = WorkspaceHandle::current(cx) else { return };
    // the note's current text (unsaved edits included) if it's open
    let open = ws.0.upgrade().and_then(|w| w.read(cx).all_editors().into_iter().find(|e| ws.vault(cx).is_some_and(|v| v.borrow().relative(&e.read(cx).path) == rel)));
    let text = match open {
        Some(e) => e.read(cx).doc.text(),
        None => match ws.vault(cx).map(|v| v.borrow().root.join(&rel)).and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(t) => t,
            None => return,
        },
    };
    let Some(job) = build(&rel, &text, &page, cx) else { return };
    if let Ok(dir) = std::env::var("OBSIDILIGHT_TYPST_OUT") {
        let _ = std::fs::write(Path::new(&dir).join("main.typ"), &job.source);
    }
    let title = Path::new(&rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let state = Rc::new(RefCell::new(State::Working));
    let task = cx.background_spawn(async move { compile::run_isolated(job) });
    let st = state.clone();
    cx.spawn(async move |cx| {
        let res = task.await;
        let script = std::env::var_os("OBSIDILIGHT_SCRIPT").is_some();
        let new = match res {
            Ok(out) => {
                if script {
                    println!("PDF pages={} bytes={} math_ms={:.0} compile_ms={:.0} pdf_ms={:.0} warnings={:?}", out.pages.len(), out.pdf.len(), out.ms.0, out.ms.1, out.ms.2, out.warnings);
                    if let Ok(p) = std::env::var("OBSIDILIGHT_PDF_OUT") {
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
        *st.borrow_mut() = new;
        let _ = cx.update(|cx| cx.refresh_windows());
    })
    .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let s = state.borrow();
        let (body, pdf): (AnyElement, Option<Vec<u8>>) = match &*s {
            State::Working => (div().p_8().text_color(theme.muted_foreground).child("Typesetting…").into_any_element(), None),
            State::Failed(e) => (div().p_4().text_color(theme.danger).child(format!("Could not typeset the note: {e}")).into_any_element(), None),
            State::Done(out, imgs) => {
                // only the pages in view are drawn (decoded), a long note stays cheap
                let imgs = imgs.clone();
                let page_h = 560. * 297. / 210.;
                let pages = uniform_list("pdf-pages", imgs.len(), move |range, _, _| {
                    range.map(|i| div().w_full().h(px(page_h + 12.)).flex().justify_center().child(div().w(px(560.)).h(px(page_h)).bg(gpui_kit::white()).shadow_md().child(img(imgs[i].clone()).size_full()))).collect::<Vec<_>>()
                });
                let info = format!("{} page{} · {} KB", out.pages.len(), if out.pages.len() == 1 { "" } else { "s" }, out.pdf.len() / 1024);
                (
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(theme.muted_foreground).child(info))
                        .child(div().h(px(560.)).bg(theme.muted).pt_3().child(pages.size_full()))
                        .into_any_element(),
                    Some(out.pdf.clone()),
                )
            }
        };
        drop(s);
        let title = title.clone();
        let save = Button::new("pdf-save").primary().label("Save PDF…").disabled(pdf.is_none()).on_click(move |_, window, cx| {
            let Some(bytes) = pdf.clone() else { return };
            let dir = std::env::var_os("HOME").map(PathBuf::from).map(|h| if h.join("Documents").is_dir() { h.join("Documents") } else { h }).unwrap_or_else(|| PathBuf::from("."));
            let rx = cx.prompt_for_new_path(&dir, Some(&format!("{title}.pdf")));
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
        });
        dialog.title("Export to PDF").w(px(640.)).child(body).footer(h_flex().gap_2().justify_end().child(Button::new("pdf-cancel").outline().label("Close").on_click(|_, window, cx| window.close_dialog(cx))).child(save))
    });
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
                    SettingSpec { key: "marginMm", label: "Margins (mm)", description: "All four sides.", kind: SettingKind::Number, default: Value::from(margin) },
                    SettingSpec { key: "showTitle", label: "Title", description: "Start with the note's name as a heading.", kind: SettingKind::Toggle, default: b("showTitle") },
                    SettingSpec { key: "displayHeader", label: "Header", description: "The note's name at the top of every page.", kind: SettingKind::Toggle, default: b("displayHeader") },
                    SettingSpec { key: "displayFooter", label: "Footer", description: "\"page / total\" at the bottom of every page.", kind: SettingKind::Toggle, default: b("displayFooter") },
                    SettingSpec { key: "fontSize", label: "Font size (pt)", description: "", kind: SettingKind::Number, default: Value::from(11.0) },
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
        let page = page_of(settings);
        let p1 = page.clone();
        let run: Action = Rc::new(move |a: &mut ActionCtx| {
            if let Some(rel) = a.workspace().and_then(|w| w.active_note(a.cx)) {
                export(rel, p1.clone(), a.window, a.cx);
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
                    export(rel.to_string(), page.clone(), a.window, a.cx);
                }
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{prelude, Page};

    #[test]
    fn prelude_has_page_header_footer() {
        let p = Page { paper: "A4".into(), margin_mm: 10.0, title: true, header: true, footer: true, font_pt: 11.0 };
        let t = prelude("Note \"x\"", &p);
        assert!(t.contains("paper: \"a4\", margin: 10mm"));
        assert!(t.contains("#counter(page).display() / #counter(page).final().first()"));
        assert!(t.contains("#set document(title: \"Note \\\"x\\\"\")"));
    }
}
