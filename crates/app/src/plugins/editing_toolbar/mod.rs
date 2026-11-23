//! Editing toolbar (replaces the community plugin "Editing Toolbar"): a Word-like formatting
//! bar above the editor. Its layout is the one saved by the Obsidian plugin
//! (`.obsidian/plugins/editing-toolbar/data.json`, `menuCommands`; read, never written), with
//! the same command ids, so buttons, submenus and dropdowns come out as you arranged them.
//! Every action is also a command (palette, hotkeys: Ctrl+B / Ctrl+I / Ctrl+K as in Obsidian).

pub mod ops;

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::*;
use obsidilight_core::EditKind;
use serde_json::{Map, Value};

use self::ops::{Change, List};
use super::api::*;

/// What an action does.
#[derive(Clone)]
enum Op {
    Wrap(&'static str, &'static str),
    /// Wrap with HTML whose opening tag depends on settings (colours).
    WrapWith(String, &'static str),
    Heading(usize),
    List(List),
    Checklist,
    CycleList,
    Indent,
    Outdent,
    Quote,
    Block(&'static str, &'static str),
    InsertLine(&'static str),
    Link(&'static str),
    Swap(i32),
    Clear,
    Lines(&'static str),
    PlainText,
    Editor(&'static str),
    AttachFile,
    FocusMode,
}

/// (id without the `editing-toolbar:` prefix, name, default hotkey, op). Ids are the plugin's.
fn table(font: &str, bg: &str) -> Vec<(&'static str, &'static str, Option<&'static str>, Op)> {
    vec![
        ("editor-undo", "Undo", None, Op::Editor("undo")),
        ("editor-redo", "Redo", None, Op::Editor("redo")),
        ("editor-copy", "Copy", None, Op::Editor("copy")),
        ("editor-cut", "Cut", None, Op::Editor("cut")),
        ("editor-paste", "Paste", None, Op::Editor("paste")),
        ("format-eraser", "Clear text formatting", None, Op::Clear),
        ("header1-text", "Heading 1", None, Op::Heading(1)),
        ("header2-text", "Heading 2", None, Op::Heading(2)),
        ("header3-text", "Heading 3", None, Op::Heading(3)),
        ("header4-text", "Heading 4", None, Op::Heading(4)),
        ("header5-text", "Heading 5", None, Op::Heading(5)),
        ("header6-text", "Heading 6", None, Op::Heading(6)),
        ("toggle-bold", "Bold", Some("ctrl-b"), Op::Wrap("**", "**")),
        ("toggle-italics", "Italic", Some("ctrl-i"), Op::Wrap("*", "*")),
        ("toggle-strikethrough", "Strikethrough", None, Op::Wrap("~~", "~~")),
        ("underline", "Underline", None, Op::Wrap("<u>", "</u>")),
        ("toggle-highlight", "Highlight", None, Op::Wrap("==", "==")),
        ("superscript", "Superscript", None, Op::Wrap("<sup>", "</sup>")),
        ("subscript", "Subscript", None, Op::Wrap("<sub>", "</sub>")),
        ("editor:toggle-code", "Inline code", None, Op::Wrap("`", "`")),
        ("toggle-inline-math", "Inline math", None, Op::Wrap("$", "$")),
        ("get-plain-text", "Copy as plain text", None, Op::PlainText),
        ("insert-blank-lines", "Insert blank lines", None, Op::Lines("insert-blank-lines")),
        ("remove-blank-lines", "Remove blank lines", None, Op::Lines("remove-blank-lines")),
        ("split-lines", "Split lines at sentences", None, Op::Lines("split-lines")),
        ("merge-lines", "Merge lines", None, Op::Lines("merge-lines")),
        ("dedupe-lines", "Remove duplicate lines", None, Op::Lines("dedupe-lines")),
        ("number-lines", "Number lines", None, Op::Lines("number-lines")),
        ("remove-whitespace-trim", "Trim line ends", None, Op::Lines("remove-whitespace-trim")),
        ("remove-whitespace-compress", "Shrink extra spaces", None, Op::Lines("remove-whitespace-compress")),
        ("remove-whitespace-all", "Remove all whitespace", None, Op::Lines("remove-whitespace-all")),
        ("list-to-table", "List to table", None, Op::Lines("list-to-table")),
        ("table-to-list", "Table to list", None, Op::Lines("table-to-list")),
        ("editor:swap-line-down", "Move line down", None, Op::Swap(1)),
        ("editor:swap-line-up", "Move line up", None, Op::Swap(-1)),
        ("editor:attach-file", "Attach file", None, Op::AttachFile),
        ("editor:insert-table", "Insert table", None, Op::InsertLine("\n| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n")),
        ("editor:cycle-list-checklist", "Cycle list / checklist", None, Op::CycleList),
        ("editor:toggle-blockquote", "Blockquote", None, Op::Quote),
        ("insert-callout", "Insert callout", None, Op::Block("> [!note]", "")),
        ("codeblock", "Code block", None, Op::Block("```", "```")),
        ("editor:insert-wikilink", "Insert wikilink", None, Op::Link("wikilink")),
        ("editor:insert-embed", "Insert embed", None, Op::Link("embed")),
        ("insert-link", "Insert link", Some("ctrl-k"), Op::Link("md")),
        ("hrline", "Horizontal divider", None, Op::InsertLine("\n---\n")),
        ("editor:insert-mathblock", "Math block", None, Op::Block("$$", "$$")),
        ("editor:toggle-checklist-status", "Toggle checklist status", Some("ctrl-l"), Op::Checklist),
        ("toggle-numbered-list", "Numbered list", None, Op::List(List::Numbered)),
        ("toggle-bullet-list", "Bullet list", None, Op::List(List::Bullet)),
        ("undent-list", "Unindent list", None, Op::Outdent),
        ("indent-list", "Indent list", None, Op::Indent),
        ("justify", "Justify", None, Op::Wrap("<p align=\"justify\">", "</p>")),
        ("left", "Align left", None, Op::Wrap("<p align=\"left\">", "</p>")),
        ("center", "Centre", None, Op::Wrap("<center>", "</center>")),
        ("right", "Align right", None, Op::Wrap("<p align=\"right\">", "</p>")),
        ("change-font-color", "Font colour", None, Op::WrapWith(format!("<font color=\"{font}\">"), "</font>")),
        ("change-background-color", "Background colour", None, Op::WrapWith(format!("<span style=\"background:{bg}\">"), "</span>")),
        ("fullscreen-focus", "Focus mode", None, Op::FocusMode),
        ("workplace-fullscreen-focus", "Focus mode (workspace)", None, Op::FocusMode),
    ]
}

fn icon_for(id: &str) -> IconName {
    let id = id.trim_start_matches("editing-toolbar:");
    match id {
        "editor-undo" => IconName::Undo2,
        "editor-redo" => IconName::Redo2,
        "editor-copy" => IconName::Copy,
        "editor-cut" => IconName::Scissors,
        "editor-paste" => IconName::ClipboardPaste,
        "format-eraser" | "remove-whitespace-all" => IconName::Eraser,
        "header1-text" => IconName::Heading1,
        "header2-text" => IconName::Heading2,
        "header3-text" => IconName::Heading3,
        "header4-text" => IconName::Heading4,
        "header5-text" => IconName::Heading5,
        "header6-text" => IconName::Heading6,
        "toggle-bold" => IconName::Bold,
        "toggle-italics" => IconName::Italic,
        "toggle-strikethrough" => IconName::Strikethrough,
        "underline" => IconName::Underline,
        "toggle-highlight" => IconName::Highlighter,
        "superscript" => IconName::Superscript,
        "subscript" => IconName::Subscript,
        "editor:toggle-code" => IconName::Code,
        "codeblock" => IconName::SquareCode,
        "toggle-inline-math" => IconName::Sigma,
        "editor:insert-mathblock" => IconName::SquareSigma,
        "editor:attach-file" => IconName::Paperclip,
        "editor:insert-table" | "list-to-table" => IconName::Table,
        "editor:cycle-list-checklist" | "editor:toggle-checklist-status" => IconName::ListChecks,
        "editor:toggle-blockquote" => IconName::TextQuote,
        "insert-callout" => IconName::MessageSquare,
        "editor:insert-wikilink" | "insert-link" => IconName::Link,
        "editor:insert-embed" => IconName::FileText,
        "hrline" => IconName::Minus,
        "toggle-numbered-list" | "number-lines" => IconName::ListOrdered,
        "toggle-bullet-list" | "table-to-list" => IconName::List,
        "undent-list" => IconName::ListIndentDecrease,
        "indent-list" => IconName::ListIndentIncrease,
        "justify" => IconName::TextAlignJustify,
        "left" => IconName::TextAlignStart,
        "center" => IconName::TextAlignCenter,
        "right" => IconName::TextAlignEnd,
        "change-font-color" => IconName::Baseline,
        "change-background-color" => IconName::PaintBucket,
        "fullscreen-focus" | "workplace-fullscreen-focus" => IconName::Maximize,
        "editor:swap-line-down" => IconName::ArrowDown,
        "editor:swap-line-up" => IconName::ArrowUp,
        // submenus, by what they hold
        "SubmenuCommands-header" => IconName::Heading,
        "SubmenuCommands-text-tools" => IconName::Box,
        "SubmenuCommands-list" => IconName::List,
        "SubmenuCommands-aligin" => IconName::TextAlignStart,
        _ => IconName::Pencil,
    }
}

/// An entry of the saved layout.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub children: Vec<Entry>,
}

/// The layout from the plugin's `menuCommands` (only commands we have; dividers kept in menus).
pub fn layout(menu: &Value, known: &dyn Fn(&str) -> bool) -> Vec<Entry> {
    fn rec(v: &Value, known: &dyn Fn(&str) -> bool) -> Vec<Entry> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|it| {
                let id = it["id"].as_str()?.to_string();
                let name = it["name"].as_str().unwrap_or("").to_string();
                if let Some(sub) = it.get("SubmenuCommands") {
                    let children = rec(sub, known);
                    return children.iter().any(|c| !c.id.starts_with("editingToolbar-Divider")).then_some(Entry { id, name, children });
                }
                (id.starts_with("editingToolbar-Divider") || known(id.trim_start_matches("editing-toolbar:"))).then_some(Entry { id, name, children: Vec::new() })
            })
            .collect()
    }
    rec(menu, known)
}

fn apply_change(a: &mut ActionCtx, f: &dyn Fn(&str, Range<usize>) -> Option<Change>) {
    let Some(ed) = a.editor.clone() else { return };
    ed.update(a.cx, |e, cx| {
        let text = e.doc.text();
        let sel = e.doc.selection().range();
        if let Some(c) = f(&text, sel) {
            e.edit(c.range, &c.text, EditKind::Other, cx);
            e.set_selection(c.sel, false, cx);
        }
    });
}

fn run_op(op: &Op, a: &mut ActionCtx) {
    match op {
        Op::Wrap(o, c) => apply_change(a, &|t, s| Some(ops::toggle_wrap(t, s, o, c))),
        Op::WrapWith(o, c) => apply_change(a, &|t, s| Some(ops::toggle_wrap(t, s, o, c))),
        Op::Heading(n) => apply_change(a, &|t, s| Some(ops::heading(t, s, *n))),
        Op::List(k) => apply_change(a, &|t, s| Some(ops::toggle_list(t, s, *k))),
        Op::Checklist => apply_change(a, &|t, s| Some(ops::toggle_checklist(t, s))),
        Op::CycleList => apply_change(a, &|t, s| Some(ops::cycle_list(t, s))),
        Op::Indent => apply_change(a, &|t, s| Some(ops::indent(t, s))),
        Op::Outdent => apply_change(a, &|t, s| Some(ops::outdent(t, s))),
        Op::Quote => apply_change(a, &|t, s| Some(ops::toggle_quote(t, s))),
        Op::Block(o, c) => apply_change(a, &|t, s| Some(ops::block(t, s, o, c))),
        Op::InsertLine(x) => apply_change(a, &|t, s| Some(ops::insert_after_line(t, s, x))),
        Op::Link(k) => apply_change(a, &|t, s| Some(ops::link(t, s, k))),
        Op::Swap(d) => apply_change(a, &|t, s| ops::swap_lines(t, s, *d)),
        Op::Clear => apply_change(a, &|t, s| Some(ops::clear_formatting(t, s))),
        Op::Lines(tool) => apply_change(a, &|t, s| ops::lines_tool(t, s, tool)),
        Op::PlainText => {
            if let Some(ed) = a.editor.clone() {
                let s = {
                    let e = ed.read(a.cx);
                    e.doc.slice(e.doc.selection().range())
                };
                a.cx.write_to_clipboard(ClipboardItem::new_string(ops::plain_text(&s)));
            }
        }
        Op::Editor(what) => {
            if let Some(ed) = a.editor.clone() {
                let f = ed.read(a.cx).focus.clone();
                a.window.focus(&f, a.cx);
                let action: Box<dyn gpui_kit::Action> = match *what {
                    "undo" => Box::new(crate::editor::Undo),
                    "redo" => Box::new(crate::editor::Redo),
                    "copy" => Box::new(crate::editor::Copy),
                    "cut" => Box::new(crate::editor::Cut),
                    _ => Box::new(crate::editor::Paste),
                };
                a.window.dispatch_action(action, a.cx);
            }
        }
        Op::AttachFile => {
            let Some(ed) = a.editor.clone() else { return };
            let paths = a.cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Attach".into()) });
            a.window
                .spawn(a.cx, async move |cx| {
                    let Ok(Ok(Some(paths))) = paths.await else { return };
                    let _ = ed.update(cx, |e, cx| {
                        let mut embeds = Vec::new();
                        for p in &paths {
                            let imported = e.vault.borrow_mut().import_file(p);
                            if let Ok(name) = imported {
                                embeds.push(format!("![[{name}]]"));
                            }
                        }
                        if !embeds.is_empty() {
                            let r = e.doc.selection().range();
                            e.edit(r, &embeds.join("\n"), EditKind::Other, cx);
                        }
                    });
                })
                .detach();
        }
        Op::FocusMode => {
            if let Some(ws) = a.workspace() {
                ws.toggle_focus_mode(a.cx);
            }
        }
    }
    // the bar's buttons may take the focus: give it back to the text
    if let Some(ed) = a.editor.clone() {
        let f = ed.read(a.cx).focus.clone();
        a.window.focus(&f, a.cx);
    }
}

pub struct EditingToolbar {
    manifest: PluginManifest,
}

impl EditingToolbar {
    pub fn new(vault: &Path) -> Self {
        let saved: Map<String, Value> = std::fs::read_to_string(vault.join(".obsidian/plugins/editing-toolbar/data.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let s = |k: &str, d: &str| Value::from(saved.get(k).and_then(Value::as_str).unwrap_or(d));
        Self {
            manifest: PluginManifest {
                id: "editing-toolbar",
                name: "Editing toolbar",
                version: "0.1.0",
                description: "A formatting toolbar above the editor, laid out as in your Editing Toolbar settings; every button is also a command.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "showToolbar", label: "Show the toolbar", description: "Off: the commands stay (palette, hotkeys) without the bar.", kind: SettingKind::Toggle, default: Value::Bool(saved.get("enableTopToolbar").and_then(Value::as_bool).unwrap_or(true)) },
                    SettingSpec { key: "cMenuFontColor", label: "Font colour", description: "Used by the font colour button.", kind: SettingKind::Text, default: s("cMenuFontColor", "#ff0000") },
                    SettingSpec { key: "cMenuBackgroundColor", label: "Background colour", description: "Used by the background colour button.", kind: SettingKind::Text, default: s("cMenuBackgroundColor", "#d3f8b6") },
                    SettingSpec { key: "menuCommands", label: "Layout", description: "", kind: SettingKind::Hidden, default: saved.get("menuCommands").cloned().unwrap_or(Value::Array(Vec::new())) },
                ],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for EditingToolbar {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str, d: &str| settings.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let ops: HashMap<&'static str, (&'static str, Op)> = table(&get("cMenuFontColor", "#ff0000"), &get("cMenuBackgroundColor", "#d3f8b6")).into_iter().map(|(id, name, hot, op)| {
            let op2 = op.clone();
            reg.command(Command { id: id.into(), name: name.into(), hotkey: hot.map(String::from), run: Rc::new(move |a: &mut ActionCtx| run_op(&op2, a)) });
            (id, (name, op))
        }).collect();
        if !settings.get("showToolbar").and_then(Value::as_bool).unwrap_or(true) {
            return;
        }
        let mut entries = layout(settings.get("menuCommands").unwrap_or(&Value::Null), &|id| ops.contains_key(id));
        if entries.is_empty() {
            // no saved layout: the basics
            entries = ["toggle-bold", "toggle-italics", "toggle-strikethrough", "toggle-highlight", "header2-text", "header3-text", "toggle-bullet-list", "toggle-numbered-list", "editor:toggle-checklist-status", "insert-link", "toggle-inline-math"]
                .iter()
                .map(|id| Entry { id: id.to_string(), name: ops[id].0.to_string(), children: Vec::new() })
                .collect();
        }
        let ops = Rc::new(ops);
        let entries = Rc::new(entries);
        reg.toolbar(ToolbarSlot {
            id: "toolbar".into(),
            render: Rc::new(move |v: &ViewCtx, _window, cx| {
                let theme = cx.theme();
                let run = |id: &str| -> Rc<dyn Fn(&mut Window, &mut App)> {
                    let op = ops.get(id.trim_start_matches("editing-toolbar:")).map(|(_, o)| o.clone());
                    let ws = v.workspace.clone();
                    Rc::new(move |window, cx| {
                        let Some(op) = op.clone() else { return };
                        let editor = ws.active_editor(cx);
                        let mut a = ActionCtx { window, cx, editor };
                        run_op(&op, &mut a);
                    })
                };
                let buttons = entries.iter().enumerate().map(|(i, e)| {
                    let tip = if e.name.is_empty() || e.name.starts_with("submenu") { e.children.first().map(|c| c.name.clone()).unwrap_or_default() } else { e.name.clone() };
                    // a submenu we have no icon for shows its first command's
                    let icon = match e.children.iter().find(|c| !c.id.starts_with("editingToolbar-Divider")) {
                        Some(first) if icon_for(&e.id) == IconName::Pencil => icon_for(&first.id),
                        _ => icon_for(&e.id),
                    };
                    let b = Button::new(("et", i)).ghost().xsmall().icon(icon).tooltip(tip);
                    if e.children.is_empty() {
                        let f = run(&e.id);
                        b.on_click(move |_, window, cx| f(window, cx)).into_any_element()
                    } else {
                        let kids: Vec<(String, String, Option<Rc<dyn Fn(&mut Window, &mut App)>>)> = e
                            .children
                            .iter()
                            .map(|c| (c.id.clone(), c.name.clone(), (!c.id.starts_with("editingToolbar-Divider")).then(|| run(&c.id))))
                            .collect();
                        let ops = ops.clone();
                        b.dropdown_menu(move |mut menu, _, _| {
                            for (id, name, f) in &kids {
                                match f {
                                    None => menu = menu.separator().label(name.clone()),
                                    Some(f) => {
                                        let f = f.clone();
                                        let label = ops.get(id.trim_start_matches("editing-toolbar:")).map(|(n, _)| n.to_string()).unwrap_or_else(|| name.clone());
                                        menu = menu.item(PopupMenuItem::new(label).icon(icon_for(id)).on_click(move |_, window, cx| f(window, cx)));
                                    }
                                }
                            }
                            menu
                        })
                        .into_any_element()
                    }
                });
                h_flex().flex_wrap().gap_0p5().px_2().py_0p5().border_b_1().border_color(theme.border).bg(theme.background).children(buttons).into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::layout;
    use serde_json::json;

    #[test]
    fn layout_keeps_known_commands_submenus_and_dividers() {
        let menu = json!([
            {"id": "editing-toolbar:toggle-bold", "name": "Bold"},
            {"id": "editing-toolbar:toggle-format-brush", "name": "Format Brush"},
            {"id": "SubmenuCommands-header", "name": "submenu", "SubmenuCommands": [
                {"id": "editing-toolbar:header1-text", "name": "Header 1"}
            ]},
            {"id": "SubmenuCommands-x", "name": "Tools", "SubmenuCommands": [
                {"id": "editingToolbar-Divider-Line", "name": "Line Operations"},
                {"id": "editing-toolbar:merge-lines", "name": "Merge"},
                {"id": "editing-toolbar:smart-symbols", "name": "Unsupported"}
            ]},
            {"id": "SubmenuCommands-y", "name": "Only unsupported", "SubmenuCommands": [{"id": "editing-toolbar:add-wrap", "name": "Wrap"}]}
        ]);
        let known = |id: &str| ["toggle-bold", "header1-text", "merge-lines"].contains(&id);
        let l = layout(&menu, &known);
        let ids: Vec<&str> = l.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["editing-toolbar:toggle-bold", "SubmenuCommands-header", "SubmenuCommands-x"]);
        assert_eq!(l[2].children.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["editingToolbar-Divider-Line", "editing-toolbar:merge-lines"]);
    }
}
