//! Editing toolbar (replaces the community plugin "Editing Toolbar"): a Word-like formatting
//! bar above the editor. Its layout starts as the one saved by the Obsidian plugin
//! (`.obsidian/plugins/editing-toolbar/data.json`, `menuCommands`; read, never written), with
//! the same command ids, so buttons, submenus and dropdowns come out as you arranged them.
//! You change it in Settings → Editing toolbar (M8); that copy is saved in
//! `.obsidilight/plugins/editing-toolbar.json`.
//! Every action is also a command (palette, hotkeys: Ctrl+B / Ctrl+I / Ctrl+K as in Obsidian).
//! The colour buttons have a picker next to them; the colour you pick is applied and becomes
//! the button's colour.

pub mod layout;
mod layout_ui;
pub mod ops;

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::*;
use obsidilight_core::EditKind;
use serde_json::{Map, Value};

use self::layout::Item;
use self::ops::{Change, ColorKind, List};
use super::api::*;
use super::PluginHost;

pub const ID: &str = "editing-toolbar";

/// What an action does.
#[derive(Clone)]
enum Op {
    Wrap(&'static str, &'static str),
    /// Colour the selection with the button's current colour (again: remove it).
    Color(ColorKind),
    RemoveColor(ColorKind),
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
    /// Only the current note; `true` also full screen.
    FocusMode(bool),
}

/// (id without the `editing-toolbar:` prefix, name, default hotkey, op). Ids are the plugin's
/// (the two "remove colour" commands are ours).
fn table() -> Vec<(&'static str, &'static str, Option<&'static str>, Op)> {
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
        ("change-font-color", "Font colour", None, Op::Color(ColorKind::Font)),
        ("change-background-color", "Background colour", None, Op::Color(ColorKind::Background)),
        ("remove-font-color", "Remove font colour", None, Op::RemoveColor(ColorKind::Font)),
        ("remove-background-color", "Remove background colour", None, Op::RemoveColor(ColorKind::Background)),
        ("fullscreen-focus", "Fullscreen focus mode", None, Op::FocusMode(true)),
        ("workplace-fullscreen-focus", "Focus mode", None, Op::FocusMode(false)),
    ]
}

/// Where the layout editor lists a command.
fn category(id: &str) -> &'static str {
    match id {
        "editor-undo" | "editor-redo" | "editor-copy" | "editor-cut" | "editor-paste" | "editor:swap-line-down" | "editor:swap-line-up" => "Edit",
        "toggle-bold" | "toggle-italics" | "toggle-strikethrough" | "underline" | "toggle-highlight" | "superscript" | "subscript" | "editor:toggle-code" | "toggle-inline-math" | "format-eraser" => "Text",
        "change-font-color" | "change-background-color" | "remove-font-color" | "remove-background-color" | "justify" | "left" | "center" | "right" => "Colour and alignment",
        id if id.starts_with("header") => "Headings",
        "toggle-numbered-list" | "toggle-bullet-list" | "editor:toggle-checklist-status" | "editor:cycle-list-checklist" | "indent-list" | "undent-list" => "Lists",
        "editor:toggle-blockquote" | "insert-callout" | "codeblock" | "editor:insert-mathblock" | "editor:insert-table" | "hrline" => "Blocks",
        "editor:insert-wikilink" | "editor:insert-embed" | "insert-link" | "editor:attach-file" => "Links and files",
        "fullscreen-focus" | "workplace-fullscreen-focus" => "View",
        _ => "Text tools",
    }
}

const CATEGORIES: &[&str] = &["Text", "Colour and alignment", "Headings", "Lists", "Blocks", "Links and files", "Edit", "Text tools", "View"];

type Ops = HashMap<&'static str, (&'static str, Op)>;

fn ops_map() -> Ops {
    table().into_iter().map(|(id, name, _, op)| (id, (name, op))).collect()
}

/// The setting holding the colour a colour button applies.
fn color_key(k: ColorKind) -> &'static str {
    match k {
        ColorKind::Font => "cMenuFontColor",
        ColorKind::Background => "cMenuBackgroundColor",
    }
}

fn recent_key(k: ColorKind) -> &'static str {
    match k {
        ColorKind::Font => "recentFontColors",
        ColorKind::Background => "recentBackgroundColors",
    }
}

fn custom_key(k: ColorKind) -> &'static str {
    match k {
        ColorKind::Font => "customFontColors",
        ColorKind::Background => "customBackgroundColors",
    }
}

/// The colour the button applies now: the last one picked, at first your Editing Toolbar's.
fn current_color(cx: &App, k: ColorKind) -> String {
    let d = match k {
        ColorKind::Font => "#ff0000",
        ColorKind::Background => "#d3f8b6",
    };
    cx.try_global::<PluginHost>().and_then(|h| h.setting(ID, color_key(k)).as_str().map(String::from)).filter(|c| !c.trim().is_empty()).unwrap_or(d.into())
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect()
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque (as the Editing Toolbar writes colours).
pub fn hex(c: Hsla) -> String {
    let r = Rgba::from(c);
    let b = |x: f32| (x.clamp(0., 1.) * 255.).round() as u8;
    if r.a >= 0.998 {
        format!("#{:02x}{:02x}{:02x}", b(r.r), b(r.g), b(r.b))
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", b(r.r), b(r.g), b(r.b), b(r.a))
    }
}

/// A colour was picked: it colours the selection and becomes the button's colour (saved, with
/// the recent colours the picker offers first).
fn pick_color(k: ColorKind, c: String, window: &mut Window, cx: &mut App) {
    let mut recent = cx.try_global::<PluginHost>().map(|h| strings(&h.setting(ID, recent_key(k)))).unwrap_or_default();
    recent.retain(|x| !x.eq_ignore_ascii_case(&c));
    recent.insert(0, c.clone());
    recent.truncate(7);
    PluginHost::store_settings(cx, ID, vec![(color_key(k), Value::from(c.clone())), (recent_key(k), Value::from(recent))]);
    let editor = WorkspaceHandle::current(cx).and_then(|w| w.active_editor(cx));
    let mut a = ActionCtx { window, cx, editor };
    apply_change(&mut a, &|t, s| ops::set_color(t, s, k, Some(&c), false));
    focus_editor(&mut a);
}

/// The colour pickers next to the colour buttons (one per kind, kept for the window's life).
struct Pickers {
    font: Entity<ColorPickerState>,
    bg: Entity<ColorPickerState>,
    _subs: Vec<Subscription>,
}
impl Global for Pickers {}

fn make_picker(k: ColorKind, window: &mut Window, cx: &mut App, subs: &mut Vec<Subscription>) -> Entity<ColorPickerState> {
    let init = crate::css::parse_color(&current_color(cx, k));
    let state = cx.new(|cx| {
        let s = ColorPickerState::new(window, cx);
        match init {
            Some(c) => s.default_value(c),
            None => s,
        }
    });
    // Palette clicks and Enter in the hex field close the picker and apply at once; the HSLA
    // sliders change the colour continuously, so that colour is applied once the picker closes.
    let pending: Rc<Cell<Option<Hsla>>> = Rc::default();
    let p = pending.clone();
    subs.push(window.subscribe(&state, cx, move |state, ev: &ColorPickerEvent, window, cx| {
        let ColorPickerEvent::Change(Some(c)) = ev else { return };
        if state.read(cx).is_open() {
            p.set(Some(*c));
        } else {
            p.set(None);
            pick_color(k, hex(*c), window, cx);
        }
    }));
    subs.push(window.observe(&state, cx, move |state, window, cx| {
        if !state.read(cx).is_open() {
            if let Some(c) = pending.take() {
                pick_color(k, hex(c), window, cx);
            }
        }
    }));
    state
}

fn picker(k: ColorKind, window: &mut Window, cx: &mut App) -> Entity<ColorPickerState> {
    if cx.try_global::<Pickers>().is_none() {
        let mut subs = Vec::new();
        let font = make_picker(ColorKind::Font, window, cx, &mut subs);
        let bg = make_picker(ColorKind::Background, window, cx, &mut subs);
        cx.set_global(Pickers { font, bg, _subs: subs });
    }
    let p = cx.global::<Pickers>();
    let state = match k {
        ColorKind::Font => p.font.clone(),
        ColorKind::Background => p.bg.clone(),
    };
    // the colour may have changed elsewhere (Settings): show it as the picker's current one
    let cur = crate::css::parse_color(&current_color(cx, k));
    let (open, value) = {
        let s = state.read(cx);
        (s.is_open(), s.value())
    };
    if let Some(cur) = cur {
        if !open && value.map(hex) != Some(hex(cur)) {
            state.update(cx, |s, cx| s.set_value(cur, window, cx));
        }
    }
    state
}

/// The picker's first row: your Editing Toolbar colours, then the ones you picked lately.
fn featured(k: ColorKind, cx: &App) -> Vec<Hsla> {
    let Some(h) = cx.try_global::<PluginHost>() else { return Vec::new() };
    let mut out: Vec<Hsla> = Vec::new();
    for c in strings(&h.setting(ID, custom_key(k))).into_iter().chain(strings(&h.setting(ID, recent_key(k)))) {
        if let Some(c) = crate::css::parse_color(&c) {
            if !out.iter().any(|x| hex(*x) == hex(c)) {
                out.push(c);
            }
        }
    }
    out.truncate(12);
    out
}

/// Opens Settings at this plugin's page, where the layout is edited.
pub fn open_customise(window: &mut Window, cx: &mut App) {
    crate::settings_ui::open_at("Editing toolbar", window, cx);
}

/// A saved name without markup and symbols ("Change font color[html]" → "Change font color").
fn clean_name(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            '[' | ']' | '(' | ')' | '=' | '!' | '*' | '`' | '~' | '$' => {}
            _ => out.push(c),
        }
    }
    let t = out.replace("html", "").split_whitespace().collect::<Vec<_>>().join(" ");
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A submenu's tooltip: its saved name when it has one ("Text Tools"), else what it holds.
fn submenu_title(name: &str, ids: &[String]) -> String {
    let has = |p: &[&str]| ids.iter().any(|i| p.iter().any(|x| i.contains(x)));
    if !name.is_empty() && !name.starts_with("submenu") {
        return clean_name(name);
    }
    if has(&["header"]) {
        "Headings".into()
    } else if has(&["justify", "center"]) {
        "Alignment".into()
    } else if has(&["list", "checklist"]) {
        "Lists".into()
    } else if has(&["blockquote", "callout"]) {
        "Quote and callout".into()
    } else if has(&["editor-copy", "editor-cut", "editor-paste"]) {
        "Edit".into()
    } else if has(&["superscript", "subscript", "code", "link", "math"]) {
        "Formatting and links".into()
    } else {
        "More".into()
    }
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
        "fullscreen-focus" => IconName::Maximize,
        "workplace-fullscreen-focus" => IconName::Focus,
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
        Op::Color(k) => {
            let c = current_color(a.cx, *k);
            apply_change(a, &|t, s| ops::set_color(t, s, *k, Some(&c), true))
        }
        Op::RemoveColor(k) => apply_change(a, &|t, s| ops::set_color(t, s, *k, None, false)),
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
        Op::FocusMode(fullscreen) => {
            if let Some(ws) = a.workspace() {
                ws.toggle_focus_mode(*fullscreen, a.window, a.cx);
            }
        }
    }
    // the bar's buttons may take the focus: give it back to the text
    focus_editor(a);
}

fn focus_editor(a: &mut ActionCtx) {
    if let Some(ed) = a.editor.clone() {
        let f = ed.read(a.cx).focus.clone();
        a.window.focus(&f, a.cx);
    }
}

/// Runs a toolbar command on the active editor.
fn runner(ops: &Rc<Ops>, id: &str, ws: &WorkspaceHandle) -> Rc<dyn Fn(&mut Window, &mut App)> {
    let op = ops.get(id.trim_start_matches("editing-toolbar:")).map(|(_, o)| o.clone());
    let ws = ws.clone();
    Rc::new(move |window, cx| {
        let Some(op) = op.clone() else { return };
        let editor = ws.active_editor(cx);
        let mut a = ActionCtx { window, cx, editor };
        run_op(&op, &mut a);
    })
}

/// A button's tooltip and icon.
fn describe(e: &Item, ops: &Ops) -> (String, IconName) {
    let name_of = |id: &str| ops.get(id).map(|(n, _)| n.to_string());
    match &e.children {
        None => (name_of(e.command_id()).unwrap_or_else(|| clean_name(&e.name)), icon_for(&e.id)),
        Some(kids) => {
            let ids: Vec<String> = kids.iter().map(|c| c.command_id().to_string()).collect();
            // a submenu we have no icon for shows its first command's
            let icon = match kids.iter().find(|c| !c.is_divider()) {
                Some(first) if icon_for(&e.id) == IconName::Pencil => icon_for(&first.id),
                _ => icon_for(&e.id),
            };
            (submenu_title(&e.name, &ids), icon)
        }
    }
}

/// The bar. `live`: buttons act on the active editor and the colour buttons get their
/// pickers; otherwise (the preview in Settings) it is only drawn.
fn bar(entries: &[Item], ops: &Rc<Ops>, ws: &WorkspaceHandle, live: bool, window: &mut Window, cx: &mut App) -> Div {
    let theme = cx.theme().clone();
    let prefix = if live { "et" } else { "etp" };
    let buttons: Vec<AnyElement> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if e.is_divider() {
                return div().w(px(1.)).h(crate::ui::r(18.)).mx_1().bg(theme.border).into_any_element();
            }
            let (tip, icon) = describe(e, ops);
            let b = crate::ui::icon_button((prefix, i), icon).tooltip(tip);
            if !live {
                return b.into_any_element();
            }
            match &e.children {
                None => {
                    let f = runner(ops, &e.id, ws);
                    let b = b.on_click(move |_, window, cx| f(window, cx));
                    let kind = match e.command_id() {
                        "change-font-color" => Some(ColorKind::Font),
                        "change-background-color" => Some(ColorKind::Background),
                        _ => None,
                    };
                    match kind {
                        None => b.into_any_element(),
                        Some(k) => {
                            // the button applies its colour (shown as a bar under the icon);
                            // the arrow picks another
                            let color = crate::css::parse_color(&current_color(cx, k)).unwrap_or(theme.foreground);
                            let state = picker(k, window, cx);
                            h_flex()
                                .child(div().relative().child(b).child(div().absolute().bottom(px(3.)).left(crate::ui::r(8.)).w(crate::ui::r(12.)).h(px(3.)).rounded(px(1.)).bg(color)))
                                .child(
                                    div()
                                        .relative()
                                        .child(ColorPicker::new(&state).icon(IconName::ChevronDown).xsmall().featured_colors(featured(k, cx)))
                                        .children(crate::script::probe(|| format!("tb-picker:{}", color_key(k)), cx)),
                                )
                                .into_any_element()
                        }
                    }
                }
                Some(kids) => {
                    let kids: Vec<(String, String, Option<Rc<dyn Fn(&mut Window, &mut App)>>)> =
                        kids.iter().map(|c| (c.id.clone(), c.name.clone(), (!c.is_divider()).then(|| runner(ops, &c.id, ws)))).collect();
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
            }
        })
        .collect();
    h_flex().flex_wrap().items_center().gap_0p5().children(buttons)
}

/// Your layout: the app's own copy once you changed it, else the Editing Toolbar's.
fn saved_layout(settings: &Map<String, Value>) -> Vec<Item> {
    layout::parse(settings.get("menuCommands").unwrap_or(&Value::Null))
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
        let customs = |p: &str| Value::from((1..=5).filter_map(|i| saved.get(&format!("{p}{i}")).and_then(Value::as_str).map(String::from)).collect::<Vec<_>>());
        Self {
            manifest: PluginManifest {
                id: ID,
                name: "Editing toolbar",
                version: "0.2.0",
                description: "A formatting toolbar above the editor, laid out as in your Editing Toolbar settings until you change it here; every button is also a command.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "showToolbar", label: "Show the toolbar", description: "Off: the commands stay (palette, hotkeys) without the bar.", kind: SettingKind::Toggle, default: Value::Bool(saved.get("enableTopToolbar").and_then(Value::as_bool).unwrap_or(true)) },
                    SettingSpec { key: "cMenuFontColor", label: "Font colour", description: "What the font colour button applies. The arrow next to the button picks another, which then becomes this colour.", kind: SettingKind::Text, default: s("cMenuFontColor", "#ff0000") },
                    SettingSpec { key: "cMenuBackgroundColor", label: "Background colour", description: "What the background colour button applies; the arrow next to it picks another.", kind: SettingKind::Text, default: s("cMenuBackgroundColor", "#d3f8b6") },
                    SettingSpec { key: "menuCommands", label: "Layout", description: "", kind: SettingKind::Hidden, default: saved.get("menuCommands").cloned().unwrap_or(Value::Array(Vec::new())) },
                    SettingSpec { key: "customFontColors", label: "Your font colours", description: "", kind: SettingKind::Hidden, default: customs("custom_fc") },
                    SettingSpec { key: "customBackgroundColors", label: "Your background colours", description: "", kind: SettingKind::Hidden, default: customs("custom_bg") },
                    SettingSpec { key: "recentFontColors", label: "", description: "", kind: SettingKind::Hidden, default: Value::Array(Vec::new()) },
                    SettingSpec { key: "recentBackgroundColors", label: "", description: "", kind: SettingKind::Hidden, default: Value::Array(Vec::new()) },
                ],
                code_languages: &[],
            },
        }
    }
}

/// With no layout at all: the basics.
const BASICS: &[&str] = &["toggle-bold", "toggle-italics", "toggle-strikethrough", "toggle-highlight", "header2-text", "header3-text", "toggle-bullet-list", "toggle-numbered-list", "editor:toggle-checklist-status", "insert-link", "toggle-inline-math"];

impl Plugin for EditingToolbar {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        for (id, name, hot, op) in table() {
            reg.command(Command { id: id.into(), name: name.into(), hotkey: hot.map(String::from), run: Rc::new(move |a: &mut ActionCtx| run_op(&op, a)) });
        }
        reg.command(Command { id: "customize".into(), name: "Customise toolbar".into(), hotkey: None, run: Rc::new(|a: &mut ActionCtx| open_customise(a.window, a.cx)) });
        reg.settings_tab(SettingsTab { id: "layout".into(), title: "Toolbar layout".into(), render: Rc::new(layout_ui::render) });
        if !settings.get("showToolbar").and_then(Value::as_bool).unwrap_or(true) {
            return;
        }
        let ops = Rc::new(ops_map());
        let mut entries = layout::shown(&saved_layout(settings), &|id| ops.contains_key(id));
        if entries.is_empty() {
            entries = BASICS.iter().map(|id| Item::command(id, ops[id].0)).collect();
        }
        let entries = Rc::new(entries);
        reg.toolbar(ToolbarSlot {
            id: "toolbar".into(),
            render: Rc::new(move |v: &ViewCtx, window, cx| {
                let theme = cx.theme().clone();
                bar(&entries, &ops, &v.workspace, true, window, cx)
                    .id("editing-toolbar")
                    .px_2()
                    .py_0p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .context_menu(|menu, _, _| menu.item(PopupMenuItem::new("Customise toolbar…").icon(IconName::Settings).on_click(|_, window, cx| open_customise(window, cx))))
                    .into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tooltips_have_no_markup() {
        assert_eq!(super::clean_name("Change font color[html]"), "Change font color");
        assert_eq!(super::clean_name("<p aligin=\"justify\"></p>"), "");
        assert_eq!(super::clean_name("Insert link []()"), "Insert link");
        assert_eq!(super::clean_name("==Highlight=="), "Highlight");
        assert_eq!(super::submenu_title("submenu-aligin", &["justify".into(), "center".into()]), "Alignment");
        assert_eq!(super::submenu_title("submenu", &["header1-text".into()]), "Headings");
        assert_eq!(super::submenu_title("Text Tools", &["get-plain-text".into()]), "Text Tools");
    }

    #[test]
    fn hex_round_trips_through_css() {
        for c in ["#d83931", "#cdf4698c", "#000000", "#ffffff"] {
            assert_eq!(super::hex(crate::css::parse_color(c).unwrap()), c);
        }
    }

    #[test]
    fn every_command_has_a_category_and_the_basics_exist() {
        let ops = super::ops_map();
        for (id, _) in ops.iter() {
            assert!(super::CATEGORIES.contains(&super::category(id)), "{id}");
        }
        for id in super::BASICS {
            assert!(ops.contains_key(id), "{id}");
        }
    }
}
