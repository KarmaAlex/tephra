//! Backlinks and Outgoing links (Obsidian core plugins), right sidebar views for the active note.
//! - Backlinks: *linked mentions* (notes linking here, with the line of each link) and, on
//!   request, *unlinked mentions* (the note's name written without a link).
//! - Outgoing links: what the note links to and embeds (as currently typed); links whose note
//!   doesn't exist yet are dimmed, and clicking one creates it.
//!
//! Results are cached by note, document version and index generation, so the views cost
//! nothing on frames where neither changed.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tephra_core::index::{is_note, parse_note, VaultIndex};
use tephra_core::links::link_path;
use tephra_core::search::{fold, Expr, Term};
use serde_json::{Map, Value};

use super::api::*;
use super::page_preview::PreviewOnHover as _;
use super::search::{highlighted, snippet};

/// One line to show under a note: where clicking goes, the text, what to highlight.
#[derive(Clone, Debug)]
pub struct Mention {
    pub offset: usize,
    pub text: String,
    pub hl: Vec<Range<usize>>,
}

/// Mentions grouped by note (vault path), notes sorted by name.
pub type Grouped = Vec<(String, Vec<Mention>)>;

fn line_start(text: &str, off: usize) -> usize {
    text[..off].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

fn mention(text: &str, range: Range<usize>) -> Mention {
    let (t, hl) = snippet(text, line_start(text, range.start), std::slice::from_ref(&range));
    Mention { offset: range.start, text: t, hl }
}

fn sort_by_name(g: &mut Grouped) {
    let key = |p: &str| fold(p.rsplit('/').next().unwrap_or(p));
    g.sort_by(|a, b| key(&a.0).cmp(&key(&b.0)).then(a.0.cmp(&b.0)));
}

/// Notes linking to `rel` (embeds included), with the line of each link.
pub fn linked(idx: &VaultIndex, rel: &str) -> Grouped {
    let mut out: Grouped = Vec::new();
    for b in idx.backlinks(rel) {
        if b.source == rel {
            continue; // links within the note itself
        }
        let (Some(meta), Some(text)) = (idx.note(&b.source), idx.text(&b.source)) else { continue };
        let Some(l) = meta.links.get(b.link) else { continue };
        let m = mention(text, l.range.clone());
        match out.iter_mut().find(|(s, _)| *s == b.source) {
            Some((_, v)) => v.push(m),
            None => out.push((b.source.clone(), vec![m])),
        }
    }
    sort_by_name(&mut out);
    out
}

/// Places in other notes where `rel`'s name appears as plain text (not inside a link).
pub fn unlinked(idx: &VaultIndex, rel: &str) -> Grouped {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let name = name.strip_suffix(".md").unwrap_or(name);
    if name.chars().count() < 3 {
        return Vec::new();
    }
    let q = Expr::Term(Term::Content(fold(name)));
    let mut out: Grouped = Vec::new();
    for hit in idx.search(&q) {
        if hit.path == rel {
            continue;
        }
        let (Some(meta), Some(text)) = (idx.note(&hit.path), idx.text(&hit.path)) else { continue };
        let mut ms = Vec::new();
        for l in &hit.lines {
            for r in &l.ranges {
                // whole words only, and not inside a link
                let before = text[..r.start].chars().next_back();
                let after = text[r.end..].chars().next();
                if before.is_some_and(char::is_alphanumeric) || after.is_some_and(char::is_alphanumeric) {
                    continue;
                }
                if meta.links.iter().any(|k| k.range.start < r.end && r.start < k.range.end) {
                    continue;
                }
                ms.push(mention(text, r.clone()));
            }
        }
        if !ms.is_empty() {
            out.push((hit.path, ms));
        }
    }
    sort_by_name(&mut out);
    out
}

/// What a note links to: (target vault path or unresolved link text, resolved?, embed?, count),
/// resolved notes first, then other files, then unresolved, each A→Z.
pub fn outgoing(idx: &VaultIndex, rel: &str, text: &str) -> Vec<(String, bool, bool, usize)> {
    let meta = parse_note(rel, text);
    let mut out: Vec<(String, bool, bool, usize)> = Vec::new();
    for l in &meta.links {
        let Some(path) = link_path(&l.target) else { continue }; // [[#heading]]: this note
        let (key, ok) = match idx.resolve(&l.target, rel) {
            Some(p) if p == rel => continue,
            Some(p) => (p, true),
            None => (path.to_string(), false),
        };
        match out.iter_mut().find(|o| o.0 == key) {
            Some(o) => {
                o.3 += 1;
                o.2 &= l.embed;
            }
            None => out.push((key, ok, l.embed, 1)),
        }
    }
    let rank = |o: &(String, bool, bool, usize)| (!o.1, !is_note(&o.0) && o.1, fold(o.0.rsplit('/').next().unwrap_or(&o.0)));
    out.sort_by_key(rank);
    out
}

fn display_name(path: &str) -> String {
    let n = path.rsplit('/').next().unwrap_or(path);
    n.strip_suffix(".md").unwrap_or(n).to_string()
}

fn section_header(id: impl Into<ElementId>, title: String, open: bool, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .id(id)
        .gap_1()
        .py_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted)
        .cursor_pointer()
        .child(gpui_kit::component::Icon::new(if open { gpui_kit::component::IconName::ChevronDown } else { gpui_kit::component::IconName::ChevronRight }).xsmall_icon())
        .child(title)
}

trait XSmall {
    fn xsmall_icon(self) -> Self;
}
impl XSmall for gpui_kit::component::Icon {
    fn xsmall_icon(self) -> Self {
        use gpui_kit::component::Sizable as _;
        self.xsmall()
    }
}

/// Mentions grouped by note: the note's name (opens it), then each line (opens it there).
fn render_grouped(prefix: &'static str, g: &Grouped, ws: &WorkspaceHandle, cx: &App) -> Vec<AnyElement> {
    let theme = cx.theme();
    let (fg, muted, mark) = (theme.foreground, theme.muted_foreground, theme.warning.opacity(0.35));
    let mut rows = Vec::new();
    for (i, (path, ms)) in g.iter().enumerate() {
        let (ws1, p1) = (ws.clone(), path.clone());
        rows.push(
            h_flex()
                .id((prefix, i * 1000))
                .gap_2()
                .pt_1()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .cursor_pointer()
                .child(div().truncate().child(display_name(path)))
                .child(div().text_xs().text_color(muted).child(ms.len().to_string()))
                .preview_on_hover(ws, path, "")
                .on_click(move |ev, w, cx| ws1.open(&p1, OpenAt::Keep, ev.modifiers().secondary(cx), w, cx))
                .into_any_element(),
        );
        for (j, m) in ms.iter().enumerate().take(50) {
            let (ws2, p2, off) = (ws.clone(), path.clone(), m.offset);
            rows.push(
                div()
                    .id((prefix, i * 1000 + j + 1))
                    .pl_3()
                    .py_0p5()
                    .text_xs()
                    .text_color(fg)
                    .cursor_pointer()
                    .rounded_sm()
                    .hover(|s| s.bg(theme.muted))
                    .child(highlighted(m.text.clone(), &m.hl, mark))
                    .on_click(move |ev, w, cx| ws2.open(&p2, OpenAt::Offset(off), ev.modifiers().secondary(cx), w, cx))
                    .into_any_element(),
            );
        }
    }
    rows
}

type CacheKey = (String, u64, u64);

pub struct Backlinks {
    manifest: PluginManifest,
}

impl Backlinks {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "backlinks",
                name: "Backlinks",
                version: "0.1.0",
                description: "Notes that link to the active note, and unlinked mentions of its name (right sidebar).",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for Backlinks {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        let cache: Rc<RefCell<Option<(CacheKey, Rc<Grouped>, Option<Rc<Grouped>>)>>> = Rc::default();
        let show_unlinked = Rc::new(Cell::new(false));
        reg.sidebar_view(SidebarView {
            id: "backlinks".into(),
            title: "Backlinks".into(),
            icon: gpui_kit::assets::IconName::ArrowDownLeft,
            side: Side::Right,
            render: Rc::new(move |v: &ViewCtx, _, cx| {
                let (Some(rel), vault) = (v.active_note.clone(), v.vault.clone()) else {
                    return div().text_sm().child("No note open").into_any_element();
                };
                let generation = vault.borrow().index.generation();
                let key = (rel.clone(), generation, 0);
                let want_unlinked = show_unlinked.get();
                let (linked_g, unlinked_g) = {
                    let mut c = cache.borrow_mut();
                    let stale = c.as_ref().is_none_or(|(k, _, _)| *k != key);
                    if stale {
                        let idx = &vault.borrow().index;
                        let g = linked(idx, &rel);
                        if std::env::var_os("TEPHRA_SCRIPT").is_some() {
                            println!("BACKLINKS {rel} linked={:?}", g.iter().map(|(p, m)| (p.as_str(), m.len())).collect::<Vec<_>>());
                        }
                        *c = Some((key.clone(), Rc::new(g), None));
                    }
                    let entry = c.as_mut().unwrap();
                    if want_unlinked && entry.2.is_none() {
                        entry.2 = Some(Rc::new(unlinked(&vault.borrow().index, &rel)));
                    }
                    (entry.1.clone(), entry.2.clone().filter(|_| want_unlinked))
                };
                let count = |g: &Grouped| g.iter().map(|(_, m)| m.len()).sum::<usize>();
                let toggle = show_unlinked.clone();
                let mut col = v_flex()
                    .id("backlinks")
                    .size_full()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .child(section_header("bl-linked", format!("Linked mentions ({})", count(&linked_g)), true, cx))
                    .children(render_grouped("bl-l", &linked_g, &v.workspace, cx))
                    .when(linked_g.is_empty(), |d| d.child(div().pl_3().text_xs().text_color(cx.theme().muted_foreground).child("No backlinks found.")));
                col = col.child(
                    section_header("bl-unlinked", match &unlinked_g {
                        Some(g) => format!("Unlinked mentions ({})", count(g)),
                        None => "Unlinked mentions".into(),
                    }, want_unlinked, cx)
                    .mt_2()
                    .on_click(move |_, _, cx| {
                        toggle.set(!toggle.get());
                        cx.refresh_windows();
                    }),
                );
                if let Some(g) = &unlinked_g {
                    col = col.children(render_grouped("bl-u", g, &v.workspace, cx));
                }
                col.into_any_element()
            }),
        });
    }
}

pub struct OutgoingLinks {
    manifest: PluginManifest,
}

impl OutgoingLinks {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "outgoing-links",
                name: "Outgoing links",
                version: "0.1.0",
                description: "Notes and files the active note links to or embeds (right sidebar).",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for OutgoingLinks {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        type Out = Rc<Vec<(String, bool, bool, usize)>>;
        let cache: Rc<RefCell<Option<(CacheKey, Out)>>> = Rc::default();
        reg.sidebar_view(SidebarView {
            id: "outgoing".into(),
            title: "Outgoing links".into(),
            icon: gpui_kit::assets::IconName::ArrowUpRight,
            side: Side::Right,
            render: Rc::new(move |v: &ViewCtx, _, cx| {
                let (Some(rel), vault, Some(ed)) = (v.active_note.clone(), v.vault.clone(), v.editor.clone()) else {
                    return div().text_sm().child("No note open").into_any_element();
                };
                let key = (rel.clone(), ed.read(cx).doc.version(), vault.borrow().index.generation());
                let links = {
                    let mut c = cache.borrow_mut();
                    if c.as_ref().is_none_or(|(k, _)| *k != key) {
                        let text = ed.read(cx).doc.text();
                        let o = outgoing(&vault.borrow().index, &rel, &text);
                        if std::env::var_os("TEPHRA_SCRIPT").is_some() {
                            println!("OUTGOING {rel} {:?}", o.iter().map(|l| (l.0.as_str(), l.1)).collect::<Vec<_>>());
                        }
                        *c = Some((key, Rc::new(o)));
                    }
                    c.as_ref().unwrap().1.clone()
                };
                let theme = cx.theme().clone();
                let rows = links.iter().enumerate().map(|(i, (target, ok, embed, n))| {
                    let (ws, t, ok) = (v.workspace.clone(), target.clone(), *ok);
                    let label = if ok && is_note(target) { display_name(target) } else { target.rsplit('/').next().unwrap_or(target).to_string() };
                    h_flex()
                        .id(("out", i))
                        .gap_2()
                        .py_0p5()
                        .text_sm()
                        .cursor_pointer()
                        .rounded_sm()
                        .hover(|s| s.bg(theme.muted))
                        .text_color(if ok { theme.foreground } else { theme.muted_foreground })
                        .child(gpui_kit::component::Icon::new(if *embed { gpui_kit::assets::IconName::Image } else { gpui_kit::assets::IconName::Link }).xsmall_icon().text_color(theme.muted_foreground))
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .when(*n > 1, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(n.to_string())))
                        .when(ok && is_note(target), |d| d.preview_on_hover(&v.workspace, target, ""))
                        .on_click(move |ev, w, cx| {
                            let new_tab = ev.modifiers().secondary(cx);
                            if ok {
                                ws.open(&t, OpenAt::Keep, new_tab, w, cx);
                            } else if let Err(e) = ws.create_note(&t, new_tab, w, cx) {
                                eprintln!("could not create {t}: {e}");
                            }
                        })
                        .into_any_element()
                });
                let unresolved = links.iter().filter(|l| !l.1).count();
                v_flex()
                    .id("outgoing")
                    .size_full()
                    .overflow_y_scroll()
                    .child(div().pb_1().text_xs().text_color(theme.muted_foreground).child(if links.is_empty() {
                        "No outgoing links.".to_string()
                    } else if unresolved > 0 {
                        format!("{} links, {} not created yet", links.len(), unresolved)
                    } else {
                        format!("{} links", links.len())
                    }))
                    .children(rows)
                    .into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{linked, outgoing, unlinked};
    use tephra_core::index::VaultIndex;

    fn vault(files: &[(&str, &str)]) -> (std::path::PathBuf, VaultIndex) {
        let root = std::env::temp_dir().join(format!("ol-links-{}-{}", std::process::id(), files.len()));
        let _ = std::fs::remove_dir_all(&root);
        for (p, t) in files {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        }
        let idx = VaultIndex::build(&root);
        (root, idx)
    }

    #[test]
    fn linked_unlinked_and_outgoing() {
        let (root, idx) = vault(&[
            ("Grafi.md", "# Grafi\n[[#Sez]] e [[Alberi]]"),
            ("A/Alberi.md", "Vedi [[Grafi#^b1]].\nI grafi sono belli. Grafi! grafico\n![[Grafi]]"),
            ("B.md", "Parlo di grafi, non di [[Grafi|altro]]."),
        ]);
        let l = linked(&idx, "Grafi.md");
        let summary: Vec<(&str, usize)> = l.iter().map(|(p, m)| (p.as_str(), m.len())).collect();
        assert_eq!(summary, vec![("A/Alberi.md", 2), ("B.md", 1)]);
        assert_eq!(&l[0].1[0].text[l[0].1[0].hl[0].clone()], "[[Grafi#^b1]]");
        let u = unlinked(&idx, "Grafi.md");
        let summary: Vec<(&str, usize)> = u.iter().map(|(p, m)| (p.as_str(), m.len())).collect();
        assert_eq!(summary, vec![("A/Alberi.md", 2), ("B.md", 1)], "whole words, links excluded, 'grafico' excluded");
        let text = std::fs::read_to_string(root.join("Grafi.md")).unwrap();
        let o = outgoing(&idx, "Grafi.md", &(text + " [[Nuova]] ![[Grafi]]"));
        assert_eq!(o, vec![("A/Alberi.md".to_string(), true, false, 1), ("Nuova".to_string(), false, false, 1)], "self links skipped");
        let _ = std::fs::remove_dir_all(root);
    }
}
