//! Daily notes (Obsidian core plugin): open today's note, named with a moment.js format
//! (yours: `DD-MM-YY`), creating it from a template the first time; previous/next daily note.
//! Defaults come from `.obsidian/daily-notes.json` (read, never written).

use std::path::Path;
use std::rc::Rc;

use chrono::{Duration, NaiveDateTime};
use gpui_kit::component::WindowExt as _;
use serde_json::{Map, Value};

use super::api::*;
use super::templates::{fill, obsidian_core_settings};

pub struct DailyNotes {
    manifest: PluginManifest,
}

impl DailyNotes {
    pub fn new(vault: &Path) -> Self {
        let saved = obsidian_core_settings(vault, "daily-notes.json");
        let s = |k: &str, d: &str| Value::from(saved.get(k).and_then(Value::as_str).unwrap_or(d));
        Self {
            manifest: PluginManifest {
                id: "daily-notes",
                name: "Daily notes",
                version: "0.1.0",
                description: "Open today's note (created from a template the first time), and step to the previous or next one.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "format", label: "Date format", description: "The note's name (moment.js tokens, e.g. YYYY-MM-DD).", kind: SettingKind::Text, default: Value::from(saved.get("format").and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or("YYYY-MM-DD")) },
                    SettingSpec { key: "folder", label: "New file location", description: "Folder for new daily notes (vault path; empty = the vault root).", kind: SettingKind::Text, default: s("folder", "") },
                    SettingSpec { key: "template", label: "Template file", description: "Vault path of a note to start from ({{date}}, {{title}}… are filled in).", kind: SettingKind::Text, default: s("template", "") },
                ],
                code_languages: &[],
            },
        }
    }
}

/// Vault path of the daily note for `day`.
pub fn note_path(folder: &str, format: &str, day: &NaiveDateTime) -> String {
    let name = format!("{}.md", crate::moment::format(format, day));
    let folder = folder.trim().trim_matches('/');
    if folder.is_empty() { name } else { format!("{folder}/{name}") }
}

/// Existing daily notes, oldest first: (date, vault path). Looks ten years back and one ahead.
pub fn existing(index: &obsidilight_core::index::VaultIndex, folder: &str, format: &str, today: &NaiveDateTime) -> Vec<(NaiveDateTime, String)> {
    let files: std::collections::HashSet<&String> = index.files().collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for back in (-366..=3653i64).rev() {
        let d = *today - Duration::days(back);
        let p = note_path(folder, format, &d);
        if files.contains(&p) && seen.insert(p.clone()) {
            out.push((d, p));
        }
    }
    out
}

impl Plugin for DailyNotes {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str, d: &str| settings.get(k).and_then(Value::as_str).filter(|v| !v.trim().is_empty()).unwrap_or(d).to_string();
        let (format, folder, template) = (get("format", "YYYY-MM-DD"), get("folder", ""), get("template", ""));
        let (f1, d1, t1) = (format.clone(), folder.clone(), template.clone());
        let open_today: Action = Rc::new(move |a: &mut ActionCtx| {
            let Some(ws) = a.workspace() else { return };
            let now = crate::moment::now();
            let rel = note_path(&d1, &f1, &now);
            let text = match ws.vault(a.cx) {
                Some(v) if !t1.is_empty() && !v.borrow().root.join(&rel).exists() => {
                    let tpl = if t1.ends_with(".md") { t1.clone() } else { format!("{t1}.md") };
                    let root = v.borrow().root.clone();
                    match std::fs::read_to_string(root.join(&tpl)) {
                        Ok(t) => {
                            let title = rel.rsplit('/').next().unwrap_or(&rel).trim_end_matches(".md").to_string();
                            // the template plugin's formats, when it has them
                            let host = a.cx.try_global::<super::PluginHost>();
                            let date_fmt = host.and_then(|h| h.setting("templates", "dateFormat").as_str().map(String::from)).filter(|s| !s.is_empty()).unwrap_or_else(|| f1.clone());
                            let time_fmt = host.and_then(|h| h.setting("templates", "timeFormat").as_str().map(String::from)).filter(|s| !s.is_empty()).unwrap_or_else(|| "HH:mm".into());
                            fill(&t, &title, &date_fmt, &time_fmt, &now)
                        }
                        Err(_) => {
                            a.window.push_notification(format!("Daily note template \"{tpl}\" not found; created an empty note."), a.cx);
                            String::new()
                        }
                    }
                }
                _ => String::new(),
            };
            if let Err(e) = ws.create_note_with(&rel, &text, false, a.window, a.cx) {
                a.window.push_notification(format!("Could not create the daily note: {e}"), a.cx);
            }
        });
        reg.command(Command { id: "today".into(), name: "Open today's daily note".into(), hotkey: None, run: open_today.clone() });
        reg.ribbon(RibbonItem { id: "today".into(), icon: gpui_kit::assets::IconName::CalendarDays, tooltip: "Open today's daily note".into(), run: open_today });
        for (id, name, step) in [("previous", "Open previous daily note", -1i32), ("next", "Open next daily note", 1)] {
            let (f, d) = (format.clone(), folder.clone());
            reg.command(Command {
                id: id.into(),
                name: name.into(),
                hotkey: None,
                run: Rc::new(move |a: &mut ActionCtx| {
                    let Some(ws) = a.workspace() else { return };
                    let (Some(vault), Some(cur)) = (ws.vault(a.cx), ws.active_note(a.cx)) else { return };
                    let all = existing(&vault.borrow().index, &d, &f, &crate::moment::now());
                    let Some(i) = all.iter().position(|(_, p)| *p == cur) else {
                        a.window.push_notification("This note is not a daily note.", a.cx);
                        return;
                    };
                    let j = i as i32 + step;
                    match (j >= 0).then(|| all.get(j as usize)).flatten() {
                        Some((_, p)) => ws.open(p, OpenAt::Keep, false, a.window, a.cx),
                        None => a.window.push_notification(if step < 0 { "No earlier daily note." } else { "No later daily note." }, a.cx),
                    }
                }),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_listing() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(8, 0, 0).unwrap();
        assert_eq!(note_path("", "DD-MM-YY", &day), "23-11-26.md");
        assert_eq!(note_path("/Daily/", "YYYY-MM-DD", &day), "Daily/2026-11-23.md");
        let dir = std::env::temp_dir().join(format!("ol-daily-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Daily")).unwrap();
        for n in ["Daily/2026-11-20.md", "Daily/2026-11-23.md", "Daily/2025-01-01.md", "Daily/notes.md", "2026-11-22.md"] {
            std::fs::write(dir.join(n), "").unwrap();
        }
        let index = obsidilight_core::index::VaultIndex::build(&dir);
        let found: Vec<String> = existing(&index, "Daily", "YYYY-MM-DD", &day).into_iter().map(|(_, p)| p).collect();
        assert_eq!(found, ["Daily/2025-01-01.md", "Daily/2026-11-20.md", "Daily/2026-11-23.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
