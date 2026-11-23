//! Templates (Obsidian core plugin): insert a note from the template folder at the caret, with
//! `{{title}}`, `{{date}}`, `{{time}}` and `{{date:FORMAT}}` / `{{time:FORMAT}}` filled in
//! (moment.js formats). Defaults come from `.obsidian/templates.json` (read, never written).

use std::path::Path;
use std::rc::Rc;

use chrono::NaiveDateTime;
use gpui_kit::component::WindowExt as _;
use gpui_kit::App;
use obsidilight_core::index::is_note;
use obsidilight_core::EditKind;
use serde_json::{Map, Value};

use super::api::*;
use super::picker::{self, PickItem};

pub struct Templates {
    manifest: PluginManifest,
}

/// Obsidian's settings of a core plugin (`.obsidian/<file>`), or an empty map.
pub fn obsidian_core_settings(vault: &Path, file: &str) -> Map<String, Value> {
    std::fs::read_to_string(vault.join(".obsidian").join(file)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

impl Templates {
    pub fn new(vault: &Path) -> Self {
        let saved = obsidian_core_settings(vault, "templates.json");
        let s = |k: &str, d: &str| Value::from(saved.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or(d));
        Self {
            manifest: PluginManifest {
                id: "templates",
                name: "Templates",
                version: "0.1.0",
                description: "Insert a template note at the caret, with the title, date and time filled in.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "folder", label: "Template folder", description: "Notes in this folder (vault path) are the templates.", kind: SettingKind::Text, default: Value::from(saved.get("folder").and_then(Value::as_str).unwrap_or("")) },
                    SettingSpec { key: "dateFormat", label: "Date format", description: "For {{date}} (moment.js tokens, e.g. YYYY-MM-DD).", kind: SettingKind::Text, default: s("dateFormat", "YYYY-MM-DD") },
                    SettingSpec { key: "timeFormat", label: "Time format", description: "For {{time}}.", kind: SettingKind::Text, default: s("timeFormat", "HH:mm") },
                ],
                code_languages: &[],
            },
        }
    }
}

/// Fills a template's variables (Obsidian's rules; unknown `{{…}}` stay as written).
pub fn fill(template: &str, title: &str, date_fmt: &str, time_fmt: &str, now: &NaiveDateTime) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            out.push_str(&rest[i..]);
            return out;
        };
        let var = after[..j].trim();
        let (name, fmt) = var.split_once(':').map(|(n, f)| (n.trim(), Some(f.trim()))).unwrap_or((var, None));
        let value = match name.to_ascii_lowercase().as_str() {
            "title" if fmt.is_none() => Some(title.to_string()),
            "date" => Some(crate::moment::format(fmt.unwrap_or(date_fmt), now)),
            "time" => Some(crate::moment::format(fmt.unwrap_or(time_fmt), now)),
            _ => None,
        };
        match value {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[i..i + 2 + j + 2]),
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

/// Template notes in `folder` (vault paths, sorted).
pub fn list(index: &obsidilight_core::index::VaultIndex, folder: &str) -> Vec<String> {
    let folder = folder.trim().trim_matches('/');
    let prefix = format!("{folder}/");
    let mut v: Vec<String> = index.files().filter(|f| is_note(f) && !folder.is_empty() && f.starts_with(&prefix)).cloned().collect();
    v.sort();
    v
}

impl Plugin for Templates {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str, d: &str| settings.get(k).and_then(Value::as_str).filter(|v| !v.trim().is_empty()).unwrap_or(d).to_string();
        let (folder, date_fmt, time_fmt) = (get("folder", ""), get("dateFormat", "YYYY-MM-DD"), get("timeFormat", "HH:mm"));
        // inserts `text` over the active editor's selection
        let insert = |a: &mut ActionCtx, text: &str| {
            if let Some(ed) = a.editor.clone() {
                let text = text.to_string();
                ed.update(a.cx, |e, cx| {
                    let r = e.doc.selection().range();
                    e.edit(r, &text, EditKind::Other, cx);
                });
            }
        };
        let (f1, d1, t1) = (folder.clone(), date_fmt.clone(), time_fmt.clone());
        let pick_template: Action = Rc::new(move |a: &mut ActionCtx| {
            let Some(ws) = a.workspace() else { return };
            let Some(vault) = ws.vault(a.cx) else { return };
            if f1.is_empty() {
                a.window.push_notification("Set the template folder in Settings → Templates first.", a.cx);
                return;
            }
            let files = list(&vault.borrow().index, &f1);
            if files.is_empty() {
                a.window.push_notification(format!("No templates in \"{f1}\"."), a.cx);
                return;
            }
            let root = vault.borrow().root.clone();
            let shown: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();
            let (files2, shown2, prefix) = (files.clone(), shown.clone(), format!("{}/", f1.trim_matches('/')));
            let items: picker::Items = Rc::new(move |q: &str, _: &App| {
                let mut out: Vec<(u32, String, Vec<std::ops::Range<usize>>)> = files2
                    .iter()
                    .filter_map(|f| {
                        let name = f.strip_prefix(&prefix).unwrap_or(f).trim_end_matches(".md").to_string();
                        if q.trim().is_empty() {
                            return Some((0, f.clone(), Vec::new()));
                        }
                        crate::fuzzy::score(q, &name).map(|(c, hl)| (c, f.clone(), hl))
                    })
                    .collect();
                out.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
                *shown2.borrow_mut() = out.iter().map(|o| o.1.clone()).collect();
                out.into_iter().map(|(_, f, hl)| PickItem { label: f.strip_prefix(&prefix).unwrap_or(&f).trim_end_matches(".md").to_string(), hl, ..Default::default() }).collect()
            });
            let (d2, t2) = (d1.clone(), t1.clone());
            let pick: picker::Pick = Rc::new(move |ix, _, window, cx| {
                let Some(rel) = shown.borrow().get(ix).cloned() else { return };
                let Ok(tpl) = std::fs::read_to_string(root.join(&rel)) else { return };
                let editor = WorkspaceHandle::current(cx).and_then(|w| w.active_editor(cx));
                let Some(ed) = editor else { return };
                let title = ed.read(cx).path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let text = fill(&tpl, &title, &d2, &t2, &crate::moment::now());
                ed.update(cx, |e, cx| {
                    let r = e.doc.selection().range();
                    e.edit(r, &text, EditKind::Other, cx);
                });
                let _ = window;
            });
            picker::open("Choose a template…", items, pick, a.window, a.cx);
        });
        reg.command(Command { id: "insert-template".into(), name: "Insert template".into(), hotkey: None, run: pick_template.clone() });
        reg.ribbon(RibbonItem { id: "insert-template".into(), icon: gpui_kit::assets::IconName::Files, tooltip: "Insert template".into(), run: pick_template });
        let d = date_fmt.clone();
        reg.command(Command { id: "insert-current-date".into(), name: "Insert current date".into(), hotkey: None, run: Rc::new(move |a| insert(a, &crate::moment::format(&d, &crate::moment::now()))) });
        let t = time_fmt.clone();
        reg.command(Command { id: "insert-current-time".into(), name: "Insert current time".into(), hotkey: None, run: Rc::new(move |a| insert(a, &crate::moment::format(&t, &crate::moment::now()))) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(9, 5, 0).unwrap();
        let t = "# {{title}}\n{{date}} {{time}}\n{{date:dddd}} {{ time : HH }}\n{{unknown}} {{";
        assert_eq!(fill(t, "Lecture", "DD-MM-YY", "HH:mm", &now), "# Lecture\n23-11-26 09:05\nMonday 09\n{{unknown}} {{");
    }
}
