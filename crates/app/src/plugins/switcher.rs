//! Quick switcher (Obsidian core plugin, Ctrl+O): open a note by typing part of its name.
//! Empty query: recently opened notes. Enter opens, Ctrl+Enter opens in a new tab; when no note
//! has exactly that name, the last entry creates it.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::App;
use tephra_core::index::is_note;
use serde_json::{Map, Value};

use super::api::*;
use super::picker::{self, PickItem};
use crate::fuzzy;

const LIMIT: usize = 60;

pub struct Switcher {
    manifest: PluginManifest,
}

impl Switcher {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "switcher",
                name: "Quick switcher",
                version: "0.1.0",
                description: "Open a note by name (Ctrl+O), or create it.",
                enabled_by_default: true,
                settings: vec![SettingSpec {
                    key: "showAttachments",
                    label: "Show attachments",
                    description: "List images and other files too, not just notes.",
                    kind: SettingKind::Toggle,
                    default: Value::Bool(false),
                }],
                code_languages: &[],
            },
        }
    }
}

/// What the switcher lists for `query`: vault paths, best first.
pub fn matches(files: &[String], recent: &[String], query: &str, attachments: bool) -> Vec<(String, Vec<std::ops::Range<usize>>)> {
    let name = |f: &str| -> String {
        let n = f.rsplit('/').next().unwrap_or(f);
        if is_note(n) { n[..n.len() - 3].to_string() } else { n.to_string() }
    };
    if query.trim().is_empty() {
        return recent.iter().filter(|r| files.contains(r)).take(LIMIT).map(|r| (r.clone(), Vec::new())).collect();
    }
    let mut scored: Vec<(u32, bool, usize, &String, Vec<std::ops::Range<usize>>)> = files
        .iter()
        .filter(|f| attachments || is_note(f))
        .filter_map(|f| {
            let n = name(f);
            // the name first; else the whole path (no highlight in the name then)
            match fuzzy::score(query, &n) {
                Some((c, hl)) => Some((c, !is_note(f), f.len(), f, hl)),
                None => fuzzy::score(query, f).map(|(c, _)| (c + 10, !is_note(f), f.len(), f, Vec::new())),
            }
        })
        .collect();
    scored.sort_by(|a, b| (a.0, a.1, a.2, a.3).cmp(&(b.0, b.1, b.2, b.3)));
    scored.into_iter().take(LIMIT).map(|(_, _, _, f, hl)| (f.clone(), hl)).collect()
}

impl Plugin for Switcher {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let attachments = settings.get("showAttachments").and_then(Value::as_bool).unwrap_or(false);
        let open: Action = Rc::new(move |a: &mut ActionCtx| {
            let Some(ws) = a.workspace() else { return };
            // the query and what each listed row opens (None = the "Create" row)
            let current: Rc<std::cell::RefCell<(String, Vec<Option<String>>)>> = Rc::default();
            let (ws1, cur1) = (ws.clone(), current.clone());
            let items: picker::Items = Rc::new(move |q: &str, cx: &App| {
                let Some(vault) = ws1.vault(cx) else { return Vec::new() };
                let v = vault.borrow();
                let files: Vec<String> = v.index.files().cloned().collect();
                let list = matches(&files, &ws1.recent(cx), q, attachments);
                let exact = list.iter().any(|(f, _)| {
                    let n = f.rsplit('/').next().unwrap_or(f);
                    n.trim_end_matches(".md").eq_ignore_ascii_case(q.trim())
                });
                let mut out: Vec<PickItem> = Vec::new();
                let mut keys = Vec::new();
                for (f, hl) in list {
                    let n = f.rsplit('/').next().unwrap_or(&f);
                    let label = if is_note(n) { n[..n.len() - 3].to_string() } else { n.to_string() };
                    let detail = f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                    out.push(PickItem { label, hl, detail, hint: String::new() });
                    keys.push(Some(f));
                }
                if !q.trim().is_empty() && !exact {
                    out.push(PickItem { label: format!("Create \"{}\"", q.trim()), hint: "new note".into(), ..Default::default() });
                    keys.push(None);
                }
                *cur1.borrow_mut() = (q.trim().to_string(), keys);
                out
            });
            let pick: picker::Pick = Rc::new(move |ix, new_tab, window, cx| {
                let (q, key) = {
                    let c = current.borrow();
                    (c.0.clone(), c.1.get(ix).cloned())
                };
                match key {
                    Some(Some(f)) => ws.open(&f, OpenAt::Keep, new_tab, window, cx),
                    Some(None) => {
                        if let Err(e) = ws.create_note(&q, new_tab, window, cx) {
                            window.push_notification(format!("Could not create \"{q}\": {e}"), cx);
                        }
                    }
                    None => {}
                }
            });
            picker::open("Find or create a note…", items, pick, a.window, a.cx);
        });
        reg.command(Command { id: "open".into(), name: "Open quick switcher".into(), hotkey: Some("ctrl-o".into()), run: open.clone() });
        reg.ribbon(RibbonItem { id: "open".into(), icon: gpui_kit::assets::IconName::Search, tooltip: "Quick switcher (Ctrl+O)".into(), run: open });
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn ranks_names_then_paths_notes_first() {
        let files: Vec<String> = ["Algo/Sample.md", "Algo/Graphs.md", "Images/sample.png", "Sample notes.md", "Other/Planning.md"].iter().map(|s| s.to_string()).collect();
        let got: Vec<String> = matches(&files, &[], "sam", true).into_iter().map(|(f, _)| f).collect();
        assert_eq!(got, vec!["Algo/Sample.md", "Sample notes.md", "Images/sample.png"]);
        let got: Vec<String> = matches(&files, &[], "sam", false).into_iter().map(|(f, _)| f).collect();
        assert_eq!(got, vec!["Algo/Sample.md", "Sample notes.md"]);
        let got: Vec<String> = matches(&files, &[], "algo gra", false).into_iter().map(|(f, _)| f).collect();
        assert_eq!(got, vec!["Algo/Graphs.md"], "words can match the folder");
        let recent = vec!["Other/Planning.md".to_string(), "Gone.md".to_string()];
        assert_eq!(matches(&files, &recent, "", false).len(), 1, "empty query: existing recent notes");
    }
}
