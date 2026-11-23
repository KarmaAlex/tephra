//! Note composer (Obsidian core plugin): merge a note into another, extract the selection or a
//! heading's section into a note of its own. Links follow: merging points every link at the
//! merged-into note; extracting leaves a link (or an embed) where the text was.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Window};
use obsidilight_core::index::is_note;
use obsidilight_core::markdown::{parse_blocks, BlockKind};
use obsidilight_core::EditKind;
use serde_json::{Map, Value};

use super::api::*;
use super::picker::{self, PickItem};
use super::templates::obsidian_core_settings;

pub struct NoteComposer {
    manifest: PluginManifest,
    /// Where new notes go (Obsidian's "Default location for new notes", `.obsidian/app.json`).
    new_location: (String, String),
}

impl NoteComposer {
    pub fn new(vault: &Path) -> Self {
        let app = obsidian_core_settings(vault, "app.json");
        let loc = app.get("newFileLocation").and_then(Value::as_str).unwrap_or("root").to_string();
        let folder = app.get("newFileFolderPath").and_then(Value::as_str).unwrap_or("").to_string();
        Self {
            manifest: PluginManifest {
                id: "note-composer",
                name: "Note composer",
                version: "0.1.0",
                description: "Merge notes, and extract the selection or a heading's section into a new note; links follow.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec {
                        key: "replacementText",
                        label: "Text after extraction",
                        description: "What replaces the extracted text in the original note.",
                        kind: SettingKind::Choice(vec![("link", "Link to the new note"), ("embed", "Embed the new note"), ("none", "Nothing")]),
                        default: Value::from("link"),
                    },
                    SettingSpec { key: "template", label: "Template file", description: "Vault path of a note wrapping merged or extracted text: {{content}}, {{fromTitle}}, {{newTitle}}, {{date}}.", kind: SettingKind::Text, default: Value::from("") },
                    SettingSpec { key: "confirmMerge", label: "Confirm file merge", description: "Ask before merging (the merged note goes to the trash).", kind: SettingKind::Toggle, default: Value::Bool(true) },
                ],
                code_languages: &[],
            },
            new_location: (loc, folder),
        }
    }
}

/// The section of the heading containing `caret` (or just above it): (heading line range,
/// body range up to the next heading of the same or higher level, heading text).
pub fn heading_section(text: &str, caret: usize) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>, String)> {
    let blocks = parse_blocks(text);
    let (i, level) = blocks.iter().enumerate().filter(|(_, b)| b.range.start <= caret).filter_map(|(i, b)| match b.kind {
        BlockKind::Heading { level } => Some((i, level)),
        _ => None,
    }).last()?;
    let h = &blocks[i];
    let end = blocks[i + 1..].iter().find(|b| matches!(b.kind, BlockKind::Heading { level: l } if l <= level)).map(|b| b.range.start).unwrap_or(text.len());
    // the heading line with its line break; the body starts on the next line
    let line_end = text[h.range.end..].find('\n').map(|i| h.range.end + i + 1).unwrap_or(text.len()).min(end.max(h.range.end));
    Some((h.range.start..line_end, line_end..end, text[h.content.clone()].trim().to_string()))
}

/// A note name from heading text: characters Obsidian forbids in file names removed.
pub fn safe_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']')).collect();
    t.trim().to_string()
}

/// Applies the note composer template (Obsidian's variables).
pub fn wrap(template: &str, content: &str, from: &str, new: &str) -> String {
    if template.is_empty() {
        return content.to_string();
    }
    let now = crate::moment::now();
    let t = template.replace("{{fromTitle}}", from).replace("{{newTitle}}", new);
    let t = super::templates::fill(&t, new, "YYYY-MM-DD", "HH:mm", &now);
    t.replace("{{content}}", content)
}

fn title_of(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".md").to_string()
}

/// Lets you pick a note (or type a new name, with `allow_new`); `done(vault path, is_new)`.
fn choose_note(prompt: &'static str, exclude: String, allow_new: bool, done: Rc<dyn Fn(String, bool, &mut Window, &mut App)>, ws: WorkspaceHandle, window: &mut Window, cx: &mut App) {
    let current: Rc<std::cell::RefCell<(String, Vec<Option<String>>)>> = Rc::default();
    let (ws1, cur1) = (ws.clone(), current.clone());
    let items: picker::Items = Rc::new(move |q: &str, cx: &App| {
        let Some(vault) = ws1.vault(cx) else { return Vec::new() };
        let files: Vec<String> = vault.borrow().index.files().filter(|f| is_note(f) && **f != exclude).cloned().collect();
        let list = super::switcher::matches(&files, &ws1.recent(cx), q, false);
        let exact = list.iter().any(|(f, _)| title_of(f).eq_ignore_ascii_case(q.trim()));
        let mut out = Vec::new();
        let mut keys = Vec::new();
        for (f, hl) in list {
            out.push(PickItem { label: title_of(&f), hl, detail: f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(), hint: String::new() });
            keys.push(Some(f));
        }
        if allow_new && !q.trim().is_empty() && !exact {
            out.push(PickItem { label: format!("Create \"{}\"", q.trim()), hint: "new note".into(), ..Default::default() });
            keys.push(None);
        }
        *cur1.borrow_mut() = (q.trim().to_string(), keys);
        out
    });
    let pick: picker::Pick = Rc::new(move |ix, _, window, cx| {
        let (q, key) = {
            let c = current.borrow();
            (c.0.clone(), c.1.get(ix).cloned())
        };
        match key {
            Some(Some(f)) => done(f, false, window, cx),
            Some(None) => done(q, true, window, cx),
            None => {}
        }
    });
    picker::open(prompt, items, pick, window, cx);
}

impl Plugin for NoteComposer {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let replacement = settings.get("replacementText").and_then(Value::as_str).unwrap_or("link").to_string();
        let template_path = settings.get("template").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let confirm_merge = settings.get("confirmMerge").and_then(Value::as_bool).unwrap_or(true);
        let location = self.new_location.clone();
        let template = move |ws: &WorkspaceHandle, cx: &App| -> String {
            if template_path.is_empty() {
                return String::new();
            }
            let p = if is_note(&template_path) { template_path.clone() } else { format!("{template_path}.md") };
            ws.note_text(&p, cx).unwrap_or_default()
        };
        // vault path for a new note named `name`, made from note `from`
        let new_path = move |name: &str, from: &str| -> String {
            let name = name.trim().trim_matches('/');
            if name.contains('/') {
                return name.to_string();
            }
            match location.0.as_str() {
                "current" => from.rsplit_once('/').map(|(d, _)| format!("{d}/{name}")).unwrap_or_else(|| name.to_string()),
                "folder" if !location.1.trim().is_empty() => format!("{}/{name}", location.1.trim().trim_matches('/')),
                _ => name.to_string(),
            }
        };

        // ---- merge ----
        let t1 = template.clone();
        let merge = Rc::new(move |src: String, window: &mut Window, cx: &mut App| {
            let Some(ws) = WorkspaceHandle::current(cx) else { return };
            let t1 = t1.clone();
            let exclude = src.clone();
            let done: Rc<dyn Fn(String, bool, &mut Window, &mut App)> = Rc::new(move |dst, _, window, cx| {
                let Some(ws) = WorkspaceHandle::current(cx) else { return };
                let tpl = t1(&ws, cx);
                let (src2, dst2) = (src.clone(), dst.clone());
                let run: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |window, cx| {
                    let Some(ws) = WorkspaceHandle::current(cx) else { return };
                    let (from, new) = (title_of(&src2), title_of(&dst2));
                    let tpl = tpl.clone();
                    let res = ws.merge_notes(&src2, &dst2, &move |c| wrap(&tpl, c, &from, &new), false, window, cx);
                    match res {
                        Ok(n) => window.push_notification(format!("Merged \"{}\" into \"{}\"; {n} link(s) updated.", title_of(&src2), title_of(&dst2)), cx),
                        Err(e) => window.push_notification(format!("Merge failed: {e}"), cx),
                    }
                });
                if confirm_merge {
                    let body = format!("\"{}\" will be added to the end of \"{}\" and moved to the trash. Links to it will point at \"{}\".", title_of(&src), title_of(&dst), title_of(&dst));
                    picker::confirm("Merge notes", &body, "Merge", run, window, cx);
                } else {
                    run(window, cx);
                }
            });
            choose_note("Merge into…", exclude, false, done, ws, window, cx);
        });
        let m1 = merge.clone();
        reg.command(Command {
            id: "merge-file".into(),
            name: "Merge current file with another file…".into(),
            hotkey: None,
            run: Rc::new(move |a: &mut ActionCtx| {
                let Some(src) = a.workspace().and_then(|w| w.active_note(a.cx)) else { return };
                m1(src, a.window, a.cx);
            }),
        });
        let m2 = merge.clone();
        reg.menu_item(MenuItem {
            id: "merge-file".into(),
            label: "Merge entire file with…".into(),
            target: MenuTarget::File,
            run: Rc::new(move |a: &mut ActionCtx, rel: Option<&str>| {
                if let Some(rel) = rel.filter(|r| is_note(r)) {
                    m2(rel.to_string(), a.window, a.cx);
                }
            }),
        });

        // ---- extract (selection or heading section) ----
        // moves `content` of the active note into a note (new or existing) and replaces
        // `range` with `keep` + a link
        let extract = {
            let template = template.clone();
            let new_path = new_path.clone();
            let replacement = replacement.clone();
            Rc::new(move |range: std::ops::Range<usize>, content: std::ops::Range<usize>, keep: String, default_name: Option<String>, window: &mut Window, cx: &mut App| {
                let Some(ws) = WorkspaceHandle::current(cx) else { return };
                let (Some(ed), Some(from)) = (ws.active_editor(cx), ws.active_note(cx)) else { return };
                let (template, new_path, replacement) = (template.clone(), new_path.clone(), replacement.clone());
                let from2 = from.clone();
                let done: Rc<dyn Fn(String, bool, &mut Window, &mut App)> = Rc::new(move |target, is_new, window, cx| {
                    let Some(ws) = WorkspaceHandle::current(cx) else { return };
                    let text = ed.read(cx).doc.slice(content.clone());
                    let tpl = template(&ws, cx);
                    let dst = if is_new { new_path(&target, &from) } else { target.clone() };
                    let body = wrap(&tpl, &text, &title_of(&from), &title_of(&dst));
                    let res = if is_new {
                        ws.create_file(&dst, &body, cx)
                    } else {
                        let old = ws.note_text(&dst, cx).unwrap_or_default();
                        let joined = if old.trim().is_empty() { body.clone() } else { format!("{}\n\n{}", old.trim_end_matches(['\n', '\r']), body) };
                        ws.set_note_text(&dst, &joined, cx).map(|_| dst.clone())
                    };
                    let dst = match res {
                        Ok(d) => d,
                        Err(e) => {
                            window.push_notification(format!("Could not extract: {e}"), cx);
                            return;
                        }
                    };
                    let link = ws.link_to(&dst, &from, cx);
                    let mut repl = keep.clone();
                    repl.push_str(&match replacement.as_str() {
                        "embed" => format!("![[{link}]]"),
                        "none" => String::new(),
                        _ => format!("[[{link}]]"),
                    });
                    if !keep.is_empty() && replacement != "none" {
                        repl.push('\n');
                    }
                    ed.update(cx, |e, cx| e.edit(range.clone(), &repl, EditKind::Other, cx));
                });
                match default_name {
                    Some(name) => done(name, true, window, cx),
                    None => choose_note("Extract into… (type a name for a new note)", from2, true, done, ws, window, cx),
                }
            })
        };
        let x1 = extract.clone();
        let extract_selection: Action = Rc::new(move |a: &mut ActionCtx| {
            let Some(ed) = a.editor.clone() else { return };
            let r = ed.read(a.cx).doc.selection().range();
            if r.is_empty() {
                a.window.push_notification("Select the text to extract first.", a.cx);
                return;
            }
            x1(r.clone(), r, String::new(), None, a.window, a.cx);
        });
        reg.command(Command { id: "extract-selection".into(), name: "Extract current selection…".into(), hotkey: None, run: extract_selection.clone() });
        reg.editor_menu(Rc::new(move |e, _| {
            if e.doc.selection().is_empty() {
                return Vec::new();
            }
            vec![MenuEntry { label: "Extract current selection…".into(), run: extract_selection.clone() }]
        }));
        let x2 = extract.clone();
        reg.command(Command {
            id: "extract-heading".into(),
            name: "Extract this heading…".into(),
            hotkey: None,
            run: Rc::new(move |a: &mut ActionCtx| {
                let Some(ed) = a.editor.clone() else { return };
                let (text, caret) = {
                    let e = ed.read(a.cx);
                    (e.doc.text(), e.doc.selection().head)
                };
                let Some((head, body, title)) = heading_section(&text, caret) else {
                    a.window.push_notification("The caret is not under a heading.", a.cx);
                    return;
                };
                // the heading stays; its section moves to a note named after it
                let name = safe_name(&title);
                if name.is_empty() {
                    return;
                }
                let line_end = if text[head.clone()].ends_with('\n') { "" } else { "\n" };
                x2(head.start..body.end, body.clone(), format!("{}{line_end}", &text[head.clone()]), Some(name), a.window, a.cx);
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_names_and_templates() {
        let t = "# A\nintro\n## B\nb text\n### C\nc\n## D\nd\n";
        let (h, body, title) = heading_section(t, t.find("b text").unwrap()).unwrap();
        assert_eq!((&t[h.clone()], &t[body.clone()], title.as_str()), ("## B\n", "b text\n### C\nc\n", "B"));
        assert!(heading_section("no headings", 3).is_none());
        let (_, body, _) = heading_section(t, t.find("d\n").unwrap()).unwrap();
        assert_eq!(&t[body], "d\n");
        assert_eq!(safe_name(" Lemma 1: a/b? "), "Lemma 1 ab");
        assert_eq!(wrap("", "x", "A", "B"), "x");
        assert_eq!(wrap("From [[{{fromTitle}}]] to {{newTitle}}:\n{{content}}", "body {{title}}", "A", "B"), "From [[A]] to B:\nbody {{title}}");
    }
}
