//! Outline and Tags (Obsidian core plugins), right sidebar views.
//! - Outline: the active note's headings as you type; the section holding the caret is marked;
//!   clicking a heading moves the caret there.
//! - Tags: every tag in the vault with its count, nested tags (`work/cs`) under their parent;
//!   clicking one searches for it (`tag:#work/cs`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tephra_core::markdown::{parse_blocks, BlockKind};
use tephra_core::Selection;
use serde_json::{Map, Value};

use super::api::*;

/// (level, text, byte offset) of each heading.
pub fn headings(text: &str) -> Vec<(u8, String, usize)> {
    parse_blocks(text)
        .into_iter()
        .filter_map(|b| match b.kind {
            BlockKind::Heading { level } => Some((level, text[b.content.clone()].trim().to_string(), b.range.start)),
            _ => None,
        })
        .collect()
}

/// Index of the heading whose section contains `caret`.
pub fn current(hs: &[(u8, String, usize)], caret: usize) -> Option<usize> {
    hs.iter().rposition(|h| h.2 <= caret)
}

pub struct Outline {
    manifest: PluginManifest,
}

impl Outline {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "outline",
                name: "Outline",
                version: "0.1.0",
                description: "Headings of the active note; click one to jump to it (right sidebar).",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for Outline {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        type Hs = Rc<Vec<(u8, String, usize)>>;
        let cache: Rc<RefCell<Option<((std::path::PathBuf, u64), Hs)>>> = Rc::default();
        reg.sidebar_view(SidebarView {
            id: "outline".into(),
            title: "Outline".into(),
            icon: gpui_kit::assets::IconName::ListTree,
            side: Side::Right,
            render: Rc::new(move |v: &ViewCtx, _, cx| {
                let Some(editor) = v.editor.clone() else { return div().text_sm().child("No note open").into_any_element() };
                let (key, caret) = {
                    let e = editor.read(cx);
                    ((e.path.clone(), e.doc.version()), e.doc.selection().head)
                };
                let hs = {
                    let mut c = cache.borrow_mut();
                    if c.as_ref().is_none_or(|(k, _)| *k != key) {
                        *c = Some((key, Rc::new(headings(&editor.read(cx).doc.text()))));
                    }
                    c.as_ref().unwrap().1.clone()
                };
                let theme = cx.theme().clone();
                if hs.is_empty() {
                    return div().text_xs().text_color(theme.muted_foreground).child("No headings in this note.").into_any_element();
                }
                let top = hs.iter().map(|h| h.0).min().unwrap_or(1);
                let cur = current(&hs, caret);
                let rows = hs.iter().enumerate().map(|(i, (level, text, off))| {
                    let (editor, off) = (editor.clone(), *off);
                    div()
                        .id(("outline", i))
                        .pl(crate::ui::r(14. * (level - top) as f32 + 4.))
                        .py_0p5()
                        .text_sm()
                        .truncate()
                        .cursor_pointer()
                        .rounded_sm()
                        .hover(|s| s.bg(theme.muted))
                        .when(cur == Some(i), |d| d.bg(theme.accent).text_color(theme.accent_foreground))
                        .child(text.clone())
                        .on_click(move |_, window, cx| {
                            editor.update(cx, |e, cx| {
                                window.focus(&e.focus, cx);
                                e.set_selection(Selection::caret(off), false, cx);
                            });
                        })
                });
                v_flex().id("outline").size_full().overflow_y_scroll().children(rows).into_any_element()
            }),
        });
    }
}

/// Tag tree rows: (depth, full tag, last segment, count including subtags), parents before
/// children, A→Z (or by count, highest first, within each level).
pub fn tag_rows(tags: &[(String, usize)], by_count: bool) -> Vec<(usize, String, String, usize)> {
    #[derive(Default)]
    struct Node {
        name: String,
        full: String,
        count: usize,
        kids: BTreeMap<String, Node>,
    }
    let mut root = Node::default();
    for (t, n) in tags {
        let mut node = &mut root;
        let mut full = String::new();
        for seg in t.split('/').filter(|s| !s.is_empty()) {
            if !full.is_empty() {
                full.push('/');
            }
            full.push_str(seg);
            let f = full.clone();
            node = node.kids.entry(tephra_core::search::fold(seg)).or_insert_with(|| Node { name: seg.to_string(), full: f, ..Default::default() });
            node.count += n;
        }
    }
    fn walk(n: &Node, depth: usize, by_count: bool, out: &mut Vec<(usize, String, String, usize)>) {
        let mut kids: Vec<&Node> = n.kids.values().collect();
        if by_count {
            kids.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        }
        for k in kids {
            out.push((depth, k.full.clone(), k.name.clone(), k.count));
            walk(k, depth + 1, by_count, out);
        }
    }
    let mut out = Vec::new();
    walk(&root, 0, by_count, &mut out);
    out
}

pub struct TagPane {
    manifest: PluginManifest,
}

impl TagPane {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "tag-pane",
                name: "Tags",
                version: "0.1.0",
                description: "All tags in the vault with their counts; click one to search for it (right sidebar).",
                enabled_by_default: true,
                settings: vec![SettingSpec {
                    key: "sortByCount",
                    label: "Sort by frequency",
                    description: "Most used tags first (off: A to Z).",
                    kind: SettingKind::Toggle,
                    default: Value::Bool(false),
                }],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for TagPane {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let by_count = settings.get("sortByCount").and_then(Value::as_bool).unwrap_or(false);
        type Rows = Rc<Vec<(usize, String, String, usize)>>;
        let cache: Rc<RefCell<Option<(u64, Rows)>>> = Rc::default();
        reg.sidebar_view(SidebarView {
            id: "tags".into(),
            title: "Tags".into(),
            icon: gpui_kit::assets::IconName::Tags,
            side: Side::Right,
            render: Rc::new(move |v: &ViewCtx, _, cx| {
                let vault = v.vault.clone();
                let generation = vault.borrow().index.generation();
                let rows = {
                    let mut c = cache.borrow_mut();
                    if c.as_ref().is_none_or(|(g, _)| *g != generation) {
                        *c = Some((generation, Rc::new(tag_rows(&vault.borrow().index.tags(), by_count))));
                    }
                    c.as_ref().unwrap().1.clone()
                };
                let theme = cx.theme().clone();
                if rows.is_empty() {
                    return div().text_xs().text_color(theme.muted_foreground).child("No tags found.").into_any_element();
                }
                let items = rows.iter().enumerate().map(|(i, (depth, full, name, count))| {
                    let (ws, q) = (v.workspace.clone(), format!("tag:#{full}"));
                    h_flex()
                        .id(("tag", i))
                        .pl(crate::ui::r(14. * *depth as f32 + 4.))
                        .pr_1()
                        .py_0p5()
                        .gap_2()
                        .text_sm()
                        .cursor_pointer()
                        .rounded_sm()
                        .hover(|s| s.bg(theme.muted))
                        .child(div().flex_1().min_w_0().truncate().child(format!("#{name}")))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(count.to_string()))
                        .on_click(move |_, _, cx| ws.show_view(super::search::VIEW, Some(q.clone()), cx))
                });
                v_flex().id("tags").size_full().overflow_y_scroll().children(items).into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{current, headings, tag_rows};

    #[test]
    fn headings_and_current_section() {
        let t = "# A\ntext\n## B\n```\n# not a heading\n```\n### C\n";
        let hs = headings(t);
        assert_eq!(hs.iter().map(|h| (h.0, h.1.as_str())).collect::<Vec<_>>(), vec![(1, "A"), (2, "B"), (3, "C")]);
        assert_eq!(current(&hs, 0), Some(0));
        assert_eq!(current(&hs, t.find("not").unwrap()), Some(1));
        assert_eq!(current(&hs, t.len()), Some(2));
        assert_eq!(current(&headings("x\n# A"), 0), None);
    }

    #[test]
    fn nested_tag_tree() {
        let tags = vec![("exam".to_string(), 1), ("work".to_string(), 2), ("work/cs".to_string(), 3), ("work/net".to_string(), 1)];
        let rows = tag_rows(&tags, false);
        let rows: Vec<(usize, &str, usize)> = rows.iter().map(|r| (r.0, r.2.as_str(), r.3)).collect();
        assert_eq!(rows, vec![(0, "exam", 1), (0, "work", 6), (1, "cs", 3), (1, "net", 1)]);
        let by_count: Vec<String> = tag_rows(&tags, true).into_iter().map(|r| r.1).collect();
        assert_eq!(by_count, vec!["work", "work/cs", "work/net", "exam"]);
    }
}
