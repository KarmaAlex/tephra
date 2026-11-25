//! Bookmarks (Obsidian core plugin), a left sidebar view: notes, folders, headings, blocks and
//! searches, optionally in groups. Stored in Obsidian's own format (`{"items": [{"type": "file",
//! "path": …, "ctime": …}, …]}`) in `.tephra/bookmarks.json`; the first time, Obsidian's
//! `.obsidian/bookmarks.json` is copied (read only, never written). Fields we don't use are kept.
//! Bookmarks follow renamed notes.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use serde_json::{json, Map, Value};

use super::api::*;
use super::page_preview::PreviewOnHover as _;
use super::api::MenuItem;

/// The bookmark file, as JSON (Obsidian's format).
pub struct Store {
    pub data: Value,
    path: Option<PathBuf>,
}

impl Store {
    pub fn load(vault: &Path) -> Self {
        let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(|v| v["items"].is_array());
        let ours = vault.join(".tephra/bookmarks.json");
        let data = read(ours.clone()).or_else(|| read(vault.join(".obsidian/bookmarks.json"))).unwrap_or_else(|| json!({ "items": [] }));
        Self { data, path: Some(ours) }
    }

    #[cfg(test)]
    pub fn memory(data: Value) -> Self {
        Self { data, path: None }
    }

    fn save(&self) {
        let Some(p) = &self.path else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tephra-tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&self.data).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }

    fn items(&mut self) -> &mut Vec<Value> {
        if !self.data["items"].is_array() {
            self.data["items"] = json!([]);
        }
        self.data["items"].as_array_mut().unwrap()
    }

    /// Index path of the top-level or grouped bookmark matching `f`.
    fn find(&self, f: &dyn Fn(&Value) -> bool) -> Option<Vec<usize>> {
        fn rec(items: &[Value], f: &dyn Fn(&Value) -> bool, at: &mut Vec<usize>) -> bool {
            for (i, it) in items.iter().enumerate() {
                at.push(i);
                if f(it) || it["items"].as_array().is_some_and(|sub| rec(sub, f, at)) {
                    return true;
                }
                at.pop();
            }
            false
        }
        let mut at = Vec::new();
        rec(self.data["items"].as_array()?, f, &mut at).then_some(at)
    }

    pub fn remove(&mut self, at: &[usize]) {
        let Some((last, parents)) = at.split_last() else { return };
        let mut list = self.items();
        for &i in parents {
            let Some(next) = list.get_mut(i).and_then(|g| g["items"].as_array_mut()) else { return };
            list = next;
        }
        if *last < list.len() {
            list.remove(*last);
            self.save();
        }
    }

    pub fn add(&mut self, mut item: Value) {
        item["ctime"] = json!(chrono::Local::now().timestamp_millis());
        self.items().push(item);
        self.save();
    }

    /// Adds a bookmark of `path` (file or folder), or removes it; true if it is now bookmarked.
    pub fn toggle_path(&mut self, path: &str, folder: bool) -> bool {
        let kind = if folder { "folder" } else { "file" };
        if let Some(at) = self.find(&|v| v["type"] == kind && v["path"] == path) {
            self.remove(&at);
            return false;
        }
        self.add(json!({ "type": kind, "path": path }));
        true
    }

    #[cfg(test)]
    pub fn has_file(&self, path: &str) -> bool {
        self.find(&|v| v["type"] == "file" && v["path"] == path).is_some()
    }

    /// `old` (a file or folder) moved to `new`: bookmarks of it, or of anything inside it,
    /// follow. True if any changed.
    pub fn renamed(&mut self, old: &str, new: &str) -> bool {
        fn rec(items: &mut [Value], old: &str, new: &str) -> bool {
            let mut changed = false;
            for it in items.iter_mut() {
                if let Some(p) = it["path"].as_str().map(String::from) {
                    let moved = if p == old {
                        Some(new.to_string())
                    } else {
                        p.strip_prefix(old).filter(|rest| rest.starts_with('/')).map(|rest| format!("{new}{rest}"))
                    };
                    if let Some(n) = moved {
                        it["path"] = json!(n);
                        changed = true;
                    }
                }
                if let Some(sub) = it["items"].as_array_mut() {
                    changed |= rec(sub, old, new);
                }
            }
            changed
        }
        let changed = rec(self.items(), old, new);
        if changed {
            self.save();
        }
        changed
    }
}

/// Shown title of a bookmark.
pub fn title(v: &Value) -> String {
    if let Some(t) = v["title"].as_str().filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    let path = v["path"].as_str().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.strip_suffix(".md").unwrap_or(name);
    match v["type"].as_str().unwrap_or("") {
        "search" => v["query"].as_str().unwrap_or("").to_string(),
        "heading" | "block" => format!("{name} › {}", v["subpath"].as_str().unwrap_or("").trim_start_matches('#')),
        "group" => "Group".into(),
        _ => name.to_string(),
    }
}

/// Rows to show: (depth, index path, bookmark).
pub fn flatten(data: &Value) -> Vec<(usize, Vec<usize>, Value)> {
    fn rec(items: &[Value], depth: usize, at: &mut Vec<usize>, out: &mut Vec<(usize, Vec<usize>, Value)>) {
        for (i, it) in items.iter().enumerate() {
            at.push(i);
            out.push((depth, at.clone(), it.clone()));
            if let Some(sub) = it["items"].as_array() {
                rec(sub, depth + 1, at, out);
            }
            at.pop();
        }
    }
    let mut out = Vec::new();
    if let Some(items) = data["items"].as_array() {
        rec(items, 0, &mut Vec::new(), &mut out);
    }
    out
}

fn icon(kind: &str) -> IconName {
    match kind {
        "folder" => IconName::Folder,
        "search" => IconName::Search,
        "group" => IconName::FolderOpen,
        _ => IconName::File,
    }
}

struct Listener(Rc<RefCell<Store>>);
impl VaultListener for Listener {
    fn renamed(&self, old: &str, new: &str, _: &[(String, String)], cx: &mut App) {
        if self.0.borrow_mut().renamed(old, new) {
            cx.refresh_windows();
        }
    }
}

pub struct Bookmarks {
    manifest: PluginManifest,
    vault: PathBuf,
}

impl Bookmarks {
    pub fn new(vault: &Path) -> Self {
        Self {
            manifest: PluginManifest {
                id: "bookmarks",
                name: "Bookmarks",
                version: "0.1.0",
                description: "Bookmark notes, folders, headings and searches (left sidebar).",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
            vault: vault.to_path_buf(),
        }
    }
}

impl Plugin for Bookmarks {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        let store = Rc::new(RefCell::new(Store::load(&self.vault)));
        reg.vault_listener(Rc::new(Listener(store.clone())));
        let s = store.clone();
        reg.command(Command {
            id: "toggle-file".into(),
            name: "Bookmark current file (or remove its bookmark)".into(),
            hotkey: None,
            run: Rc::new(move |a: &mut ActionCtx| {
                let Some(rel) = a.workspace().and_then(|w| w.active_note(a.cx)) else { return };
                let on = s.borrow_mut().toggle_path(&rel, false);
                if std::env::var_os("TEPHRA_SCRIPT").is_some() {
                    println!("BOOKMARK {rel} {on}");
                }
                a.cx.refresh_windows();
            }),
        });
        let s = store.clone();
        reg.command(Command {
            id: "heading".into(),
            name: "Bookmark current heading".into(),
            hotkey: None,
            run: Rc::new(move |a: &mut ActionCtx| {
                let (Some(rel), Some(ed)) = (a.workspace().and_then(|w| w.active_note(a.cx)), a.editor.clone()) else { return };
                let (text, caret) = {
                    let e = ed.read(a.cx);
                    (e.doc.text(), e.doc.selection().head)
                };
                let hs = super::outline_tags::headings(&text);
                if let Some(i) = super::outline_tags::current(&hs, caret) {
                    s.borrow_mut().add(json!({ "type": "heading", "path": rel, "subpath": format!("#{}", hs[i].1) }));
                    a.cx.refresh_windows();
                }
            }),
        });
        let s = store.clone();
        reg.menu_item(MenuItem {
            id: "toggle".into(),
            label: "Bookmark (or remove bookmark)".into(),
            target: MenuTarget::File,
            run: Rc::new(move |a: &mut ActionCtx, rel: Option<&str>| {
                let Some(rel) = rel else { return };
                let folder = a.workspace().and_then(|w| w.vault(a.cx)).is_some_and(|v| v.borrow().root.join(rel).is_dir());
                s.borrow_mut().toggle_path(rel, folder);
                a.cx.refresh_windows();
            }),
        });
        let s = store;
        reg.sidebar_view(SidebarView {
            id: "bookmarks".into(),
            title: "Bookmarks".into(),
            icon: gpui_kit::assets::IconName::Bookmark,
            side: Side::Left,
            render: Rc::new(move |v: &ViewCtx, _, cx| {
                let rows = flatten(&s.borrow().data);
                let theme = cx.theme().clone();
                if rows.is_empty() {
                    return div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("No bookmarks yet. Right-click a file and choose Bookmark, or run \"Bookmark current file\" from the command palette (Ctrl+P).")
                        .into_any_element();
                }
                let root = v.vault.borrow().root.clone();
                let exists = |p: &str| root.join(p).exists();
                let items: Vec<AnyElement> = rows
                    .into_iter()
                    .enumerate()
                    .map(|(i, (depth, at, item))| {
                        let kind = item["type"].as_str().unwrap_or("").to_string();
                        let path = item["path"].as_str().unwrap_or("").to_string();
                        let missing = !path.is_empty() && !exists(&path);
                        let (ws, s2) = (v.workspace.clone(), s.clone());
                        let item2 = item.clone();
                        h_flex()
                            .id(("bm", i))
                            .pl(crate::ui::r(14. * depth as f32 + 4.))
                            .py_0p5()
                            .gap_1p5()
                            .text_sm()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|st| st.bg(theme.muted))
                            .text_color(if missing { theme.muted_foreground } else { theme.foreground })
                            .child(gpui_kit::component::Icon::new(icon(&kind)).xsmall().text_color(theme.muted_foreground))
                            .child(div().flex_1().min_w_0().truncate().child(title(&item)))
                            .child(Button::new(("bm-remove", i)).ghost().xsmall().icon(IconName::Close).tooltip("Remove bookmark").on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                s2.borrow_mut().remove(&at);
                                cx.refresh_windows();
                            }))
                            .when(!missing && tephra_core::index::is_note(&path) && matches!(kind.as_str(), "file" | "heading" | "block"), |d| {
                                let sub = item["subpath"].as_str().unwrap_or("").trim_start_matches('#').to_string();
                                d.preview_on_hover(&v.workspace, &path, &sub)
                            })
                            .on_click(move |ev, w, cx| {
                                let new_tab = ev.modifiers().secondary(cx);
                                let path = item2["path"].as_str().unwrap_or("");
                                match kind.as_str() {
                                    "file" => ws.open(path, OpenAt::Keep, new_tab, w, cx),
                                    "heading" | "block" => ws.open(path, OpenAt::Subpath(item2["subpath"].as_str().unwrap_or("").trim_start_matches('#').to_string()), new_tab, w, cx),
                                    "search" => ws.show_view(super::search::VIEW, item2["query"].as_str().map(String::from), cx),
                                    _ => {}
                                }
                            })
                            .into_any_element()
                    })
                    .collect();
                v_flex().id("bookmarks").size_full().overflow_y_scroll().children(items).into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{flatten, title, Store};
    use serde_json::json;

    #[test]
    fn toggle_group_rename_and_titles() {
        let mut s = Store::memory(json!({ "items": [
            { "type": "group", "ctime": 1, "title": "Group", "items": [{ "type": "file", "ctime": 2, "path": "Algo/Graphs.md" }] },
            { "type": "search", "ctime": 3, "query": "tag:#work" },
            { "type": "heading", "ctime": 4, "path": "Algo/Graphs.md", "subpath": "#Intro", "extra": true },
            { "type": "folder", "ctime": 5, "path": "Algo" },
        ] }));
        assert!(s.has_file("Algo/Graphs.md"), "found inside a group");
        let titles: Vec<String> = flatten(&s.data).iter().map(|r| format!("{}{}", "  ".repeat(r.0), title(&r.2))).collect();
        assert_eq!(titles, vec!["Group", "  Graphs", "tag:#work", "Graphs › Intro", "Algo"]);
        assert!(s.renamed("Algo/Graphs.md", "Graphs/Graphs.md"));
        assert_eq!(s.data["items"][0]["items"][0]["path"], "Graphs/Graphs.md");
        assert_eq!(s.data["items"][2]["path"], "Graphs/Graphs.md");
        assert_eq!(s.data["items"][2]["extra"], true, "unknown fields kept");
        assert_eq!(s.data["items"][3]["path"], "Algo", "a file leaving a folder doesn't move the folder");
        assert!(s.renamed("Algo", "Courses"));
        assert_eq!(s.data["items"][3]["path"], "Courses", "folder moved");
        assert!(s.toggle_path("B.md", false));
        assert!(s.has_file("B.md"));
        assert!(!s.toggle_path("B.md", false));
        assert!(!s.has_file("B.md"));
        assert!(!s.renamed("Alg", "X"), "a prefix that isn't a folder boundary");
        s.remove(&[0, 0]);
        assert!(!s.has_file("Graphs/Graphs.md"));
        assert_eq!(s.data["items"][0]["items"].as_array().unwrap().len(), 0);
    }
}
