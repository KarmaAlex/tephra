//! Search (Obsidian core plugin): full-text search over the vault in the left sidebar, with
//! `"phrases"`, `-exclusion`, `OR`, `( )`, `path:`, `file:`, `tag:` and `content:`
//! (`obsidilight_core::search`). Results list each note with its matching lines; clicking one
//! opens the note there (Ctrl+click: new tab). Ctrl+Shift+F focuses it, with the selection
//! as the query.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::search::{parse_query, FileHit};
use serde_json::{Map, Value};

use super::api::*;
use super::page_preview::PreviewOnHover as _;
use crate::embeds::SharedVault;

pub const VIEW: &str = "search:search";

/// Highlights `ranges` of `text` (byte ranges) with a tinted background.
pub fn highlighted(text: impl Into<SharedString>, ranges: &[Range<usize>], color: Hsla) -> StyledText {
    let style = HighlightStyle { background_color: Some(color), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() };
    StyledText::new(text.into()).with_highlights(ranges.iter().map(|r| (r.clone(), style)))
}

/// The line around a match, shortened to fit a sidebar row, with highlights moved along.
pub fn snippet(text: &str, line_start: usize, ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let line_end = text[line_start..].find('\n').map(|i| line_start + i).unwrap_or(text.len());
    let line = text[line_start..line_end].trim_end_matches('\r');
    let first = ranges.first().map(|r| r.start - line_start).unwrap_or(0);
    // start a little before the first match, on a char boundary; skip leading blanks
    let mut from = first.saturating_sub(24);
    while !line.is_char_boundary(from) {
        from -= 1;
    }
    let lead = line[from..].len() - line[from..].trim_start().len();
    from += lead;
    let mut to = (from + 200).min(line.len());
    while !line.is_char_boundary(to) {
        to -= 1;
    }
    let prefix = if line[..from].trim().is_empty() { "" } else { "…" };
    let shown = format!("{prefix}{}{}", &line[from..to], if to < line.len() { "…" } else { "" });
    let shift = prefix.len() as isize - (line_start + from) as isize;
    let hl = ranges
        .iter()
        .filter_map(|r| {
            let (s, e) = (r.start.max(line_start + from), r.end.min(line_start + to));
            (s < e).then(|| ((s as isize + shift) as usize)..((e as isize + shift) as usize))
        })
        .collect();
    (shown, hl)
}

#[derive(Clone)]
enum Row {
    File { path: String, count: usize, collapsed: bool },
    Line { path: String, offset: usize, text: SharedString, hl: Vec<Range<usize>> },
}

pub struct SearchView {
    input: Entity<InputState>,
    vault: SharedVault,
    ws: WorkspaceHandle,
    query: String,
    hits: Vec<FileHit>,
    rows: Rc<Vec<Row>>,
    index_gen: u64,
    collapsed: HashSet<String>,
    ms: f64,
    scroll: UniformListScrollHandle,
    _sub: Subscription,
}

impl SearchView {
    fn new(vault: SharedVault, ws: WorkspaceHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search…  (\"phrase\" -not OR path: file: tag:)"));
        let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                this.run(q, cx);
            }
        });
        Self { input, vault, ws, query: String::new(), hits: Vec::new(), rows: Rc::default(), index_gen: u64::MAX, collapsed: HashSet::new(), ms: 0.0, scroll: UniformListScrollHandle::new(), _sub: sub }
    }

    fn set_query(&mut self, q: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(q.clone(), window, cx));
        self.run(q, cx);
    }

    fn run(&mut self, q: String, cx: &mut Context<Self>) {
        let t = std::time::Instant::now();
        let v = self.vault.borrow();
        self.hits = parse_query(&q).map(|e| v.index.search(&e)).unwrap_or_default();
        self.index_gen = v.index.generation();
        drop(v);
        self.ms = t.elapsed().as_secs_f64() * 1000.0;
        if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
            println!("SEARCH {:?} files={} lines={} ms={:.1}", q, self.hits.len(), self.hits.iter().map(|h| h.lines.len()).sum::<usize>(), self.ms);
        }
        self.query = q;
        self.rebuild_rows();
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        let v = self.vault.borrow();
        let mut rows = Vec::new();
        for h in &self.hits {
            let collapsed = self.collapsed.contains(&h.path);
            rows.push(Row::File { path: h.path.clone(), count: h.lines.len(), collapsed });
            if collapsed {
                continue;
            }
            let Some(text) = v.index.text(&h.path) else { continue };
            for l in &h.lines {
                let (shown, hl) = snippet(text, l.start, &l.ranges);
                rows.push(Row::Line { path: h.path.clone(), offset: l.ranges[0].start, text: shown.into(), hl });
            }
        }
        self.rows = Rc::new(rows);
    }

    /// The index changed (a save, a file from Syncthing): search again.
    fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        let generation = self.vault.borrow().index.generation();
        if generation != self.index_gen && !self.query.is_empty() {
            let q = self.query.clone();
            self.run(q, cx);
        }
    }
}

impl Render for SearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = self.rows.clone();
        let lines: usize = self.hits.iter().map(|h| h.lines.len()).sum();
        let summary = if self.query.trim().is_empty() {
            String::new()
        } else if self.hits.is_empty() {
            "No matches".into()
        } else {
            format!("{} file{}, {} line{}", self.hits.len(), if self.hits.len() == 1 { "" } else { "s" }, lines, if lines == 1 { "" } else { "s" })
        };
        let weak = cx.entity().downgrade();
        let ws = self.ws.clone();
        let mark = theme.warning.opacity(0.35);
        let (fg, muted) = (theme.foreground, theme.muted_foreground);
        v_flex()
            .size_full()
            .gap_2()
            .child(Input::new(&self.input).small().cleanable(true))
            .when(!summary.is_empty(), |d| d.child(div().text_xs().text_color(muted).child(summary)))
            .child(
                uniform_list("search-results", rows.len(), move |range, _, _| {
                    range
                        .map(|ix| {
                            let row = rows[ix].clone();
                            let ws = ws.clone();
                            let weak = weak.clone();
                            match row {
                                Row::File { path, count, collapsed } => {
                                    let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
                                    let folder = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                                    let p2 = path.clone();
                                    h_flex()
                                        .id(("sr-file", ix))
                                        .w_full()
                                        .h(px(24.))
                                        .gap_1()
                                        .cursor_pointer()
                                        .text_sm()
                                        .child(
                                            div().id(("sr-fold", ix)).child(Icon::new(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }).xsmall().text_color(muted)).on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                let p = p2.clone();
                                                let _ = weak.update(cx, |v, cx| {
                                                    if !v.collapsed.remove(&p) {
                                                        v.collapsed.insert(p);
                                                    }
                                                    v.rebuild_rows();
                                                    cx.notify();
                                                });
                                            }),
                                        )
                                        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(fg).truncate().child(name))
                                        .child(div().flex_1().min_w_0().text_xs().text_color(muted).truncate().child(folder))
                                        .when(count > 0, |d| d.child(div().text_xs().text_color(muted).child(count.to_string())))
                                        .preview_on_hover(&ws, &path, "")
                                        .on_click(move |ev, window, cx| ws.open(&path, OpenAt::Keep, ev.modifiers().secondary(cx), window, cx))
                                        .into_any_element()
                                }
                                Row::Line { path, offset, text, hl } => div()
                                    .id(("sr-line", ix))
                                    .w_full()
                                    .h(px(24.))
                                    .pl(px(18.))
                                    .text_xs()
                                    .text_color(fg)
                                    .cursor_pointer()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(highlighted(text, &hl, mark))
                                    .on_click(move |ev, window, cx| ws.open(&path, OpenAt::Offset(offset), ev.modifiers().secondary(cx), window, cx))
                                    .into_any_element(),
                            }
                        })
                        .collect()
                })
                .track_scroll(&self.scroll)
                .flex_1()
                .min_h_0(),
            )
    }
}

pub struct Search {
    manifest: PluginManifest,
}

impl Search {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "search",
                name: "Search",
                version: "0.1.0",
                description: "Search all notes (Ctrl+Shift+F): words, \"phrases\", -exclusions, OR, and path:, file:, tag:, content: filters.",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for Search {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        let view: Rc<RefCell<Option<Entity<SearchView>>>> = Rc::default();
        let focus = Rc::new(Cell::new(false));
        let f = focus.clone();
        reg.command(Command {
            id: "open".into(),
            name: "Search in all files".into(),
            hotkey: Some("ctrl-shift-f".into()),
            run: Rc::new(move |a: &mut ActionCtx| {
                // a one-line selection becomes the query, as in Obsidian
                let sel = a.editor.as_ref().map(|e| {
                    let d = &e.read(a.cx).doc;
                    d.slice(d.selection().range())
                });
                let arg = sel.filter(|s| !s.trim().is_empty() && !s.contains('\n'));
                if let Some(ws) = a.workspace() {
                    f.set(true);
                    ws.show_view(VIEW, arg, a.cx);
                }
            }),
        });
        reg.sidebar_view(SidebarView {
            id: "search".into(),
            title: "Search".into(),
            icon: gpui_kit::assets::IconName::Search,
            side: Side::Left,
            render: Rc::new(move |v: &ViewCtx, window, cx| {
                let entity = view.borrow().clone();
                let entity = match entity {
                    Some(e) => e,
                    None => {
                        let vault = v.vault.clone();
                        let ws = v.workspace.clone();
                        let e = cx.new(|cx| SearchView::new(vault, ws, window, cx));
                        *view.borrow_mut() = Some(e.clone());
                        e
                    }
                };
                entity.update(cx, |s, cx| {
                    if let Some(q) = v.arg.clone() {
                        s.set_query(q, window, cx);
                    } else {
                        s.refresh_if_stale(cx);
                    }
                    if focus.replace(false) {
                        let h = s.input.read(cx).focus_handle(cx);
                        window.focus(&h, cx);
                    }
                });
                entity.into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippets_keep_highlights_aligned() {
        let text = "intro\n    Il minimum spanning tree è unico.\n";
        let start = 6;
        let m = text.find("spanning").unwrap();
        let (s, hl) = snippet(text, start, &[m..m + 8]);
        assert_eq!(s, "Il minimum spanning tree è unico.");
        assert_eq!(&s[hl[0].clone()], "spanning");
        let long = format!("{}needle{}", "x".repeat(100), "y".repeat(300));
        let m = long.find("needle").unwrap();
        let (s, hl) = snippet(&long, 0, &[m..m + 6]);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(&s[hl[0].clone()], "needle");
    }
}
