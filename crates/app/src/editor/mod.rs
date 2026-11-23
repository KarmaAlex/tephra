//! The live-preview Markdown editor view.

pub mod api;
pub mod assist;
pub mod element;
pub mod input;
pub mod measured;
pub mod preview;
pub mod projection;
pub mod render;

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::base::text::editor_host::{LeafGeometry, RenderContext};
use gpui_kit::base::text::MarkdownExtensions;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::file::{self, DiskState, SaveOutcome};
use obsidilight_core::{Document, EditKind, Selection};

use crate::embeds::SharedVault;
use crate::plugins::{EditorHook, EditorKey, PluginHost};
pub use api::{EditorApi, TabstopSession};
use element::EditorElement;
use projection::{CodeMode, Palette, Projection, ProjectionInput};
pub use projection::ViewMode;
use std::rc::Rc;

actions!(
    editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, MoveWordLeft, MoveWordRight, MoveLineStart, MoveLineEnd, MoveDocStart, MoveDocEnd,
        SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight, SelectLineStart, SelectLineEnd,
        SelectDocStart, SelectDocEnd, SelectAll, PageUp, PageDown, Backspace, Delete, DeleteWordLeft, DeleteWordRight,
        Newline, ShiftNewline, Tab, ShiftTab, Undo, Redo, Copy, Cut, Paste, Save, ToggleReading, ToggleSource, Cancel
    ]
);

pub const CONTEXT: &str = "Editor";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, c),
        KeyBinding::new("right", MoveRight, c),
        KeyBinding::new("up", MoveUp, c),
        KeyBinding::new("down", MoveDown, c),
        KeyBinding::new("ctrl-left", MoveWordLeft, c),
        KeyBinding::new("ctrl-right", MoveWordRight, c),
        KeyBinding::new("home", MoveLineStart, c),
        KeyBinding::new("end", MoveLineEnd, c),
        KeyBinding::new("ctrl-home", MoveDocStart, c),
        KeyBinding::new("ctrl-end", MoveDocEnd, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("ctrl-shift-home", SelectDocStart, c),
        KeyBinding::new("ctrl-shift-end", SelectDocEnd, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, c),
        KeyBinding::new("ctrl-delete", DeleteWordRight, c),
        KeyBinding::new("enter", Newline, c),
        KeyBinding::new("shift-enter", ShiftNewline, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("shift-tab", ShiftTab, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("escape", Cancel, c),
        // Ctrl+S, Ctrl+E, Ctrl+Shift+E are the app commands' hotkeys (editable; see hotkeys.rs)
    ]);
}

/// Per-frame timing, for `make perf` (OBSIDILIGHT_BENCH=1).
#[derive(Default)]
pub struct Perf {
    pub pending_edit: Option<Instant>,
    /// Start of the current frame's render (for frame-work timing).
    pub render_start: Option<Instant>,
    /// Edit -> painted (includes waiting for the next frame).
    pub keystroke_ms: Vec<f64>,
    /// Render + layout + prepaint + paint of frames that show an edit.
    pub frame_work_ms: Vec<f64>,
    /// Phase timings of those frames: (render, layout, prepaint, paint) in ms.
    pub phases: Vec<[f64; 4]>,
    pub render_end: Option<Instant>,
    pub prepaint_start: Option<Instant>,
    pub projection_ms: Vec<f64>,
}

pub struct Editor {
    pub doc: Document,
    pub path: PathBuf,
    disk: Option<DiskState>,
    pub vault: SharedVault,
    pub focus: FocusHandle,
    scroll: ScrollHandle,
    proj: Projection,
    extensions: std::sync::Arc<MarkdownExtensions>,
    /// Leaf geometry from the last prepaint (window coordinates).
    pub(crate) geometry: HashMap<u64, LeafGeometry>,
    /// Remembered x for vertical motion.
    goal_x: Option<Pixels>,
    /// IME composition range (byte offsets).
    marked: Option<Range<usize>>,
    /// Scroll the caret into view on the next frame.
    pub(crate) autoscroll: bool,
    pub(crate) dragging: bool,
    pub status: Option<SharedString>,
    save_generation: u64,
    saving_disabled: bool,
    /// Measured block heights by block hash (virtualized rendering).
    heights: measured::Heights,
    /// Measured block heights by stable block id: the fallback when the hash is new (typed text),
    /// so the layout estimate doesn't jump back to a constant for a frame.
    id_heights: measured::Heights,
    measured_once: bool,
    pub mode: ViewMode,
    /// Callout fold state by callout identity (true = collapsed); absent = the file's default.
    folds: HashMap<String, bool>,
    /// Snippet tabstops (plugins create them through [`EditorApi::insert_snippet`]).
    pub tabstops: Option<TabstopSession>,
    /// `PluginHost::generation` the projection was built with.
    plugin_gen: u64,
    /// Plugin text decorations by (block text hash, decorator index, decorator revision):
    /// ranges relative to the block.
    deco_cache: HashMap<(u64, usize, u64), Rc<Vec<Range<usize>>>>,
    /// A re-render is scheduled because a decorator was still loading.
    deco_retry: bool,
    /// Plugin callout numbers for (doc version, plugin generation).
    numbers_cache: ((u64, u64), Rc<HashMap<usize, String>>),
    /// The text as a shared string, by document version (handed to code-block renderers).
    text_rc: (u64, Rc<str>),
    /// Hover preview of a link (target, mouse position, rendered snippet source + numbers).
    hover: Option<HoverPreview>,
    hover_pending: Option<String>,
    /// Link suggestions after `[[` (see `assist`).
    pub(crate) suggest: Option<SuggestState>,
    /// `[[` position whose suggestions were dismissed with Escape.
    suggest_dismissed: Option<usize>,
    suggest_key: (u64, usize),
    /// The note's `cssclasses`, by document version.
    classes: (u64, Rc<Vec<String>>),
    /// Not a note (`.css`, `.json`…): shown and edited as plain text, no Markdown.
    pub plain: bool,
    pub perf: Perf,
}

/// What an editor asks of its workspace.
#[derive(Clone, Debug)]
pub enum EditorEvent {
    /// The file was written (the index should re-read it).
    Saved(PathBuf),
    /// Ctrl+click on an internal link (`target` = link text without alias).
    OpenLink { target: String, new_tab: bool },
}

impl EventEmitter<EditorEvent> for Editor {}

impl Editor {
    pub fn open(path: PathBuf, vault: SharedVault, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (doc, disk, status) = match file::load(&path) {
            Ok(l) => (Document::new(&l.text), Some(l.disk), None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Document::new(""), None, None),
            Err(e) => (Document::new(""), None, Some(format!("Cannot open: {e}. Editing disabled for this file.").into())),
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            doc,
            path,
            disk,
            vault,
            focus,
            scroll: ScrollHandle::new(),
            proj: Projection::default(),
            extensions: crate::atoms::extensions(),
            geometry: HashMap::new(),
            goal_x: None,
            marked: None,
            autoscroll: false,
            dragging: false,
            status,
            save_generation: 0,
            saving_disabled: false,
            heights: Default::default(),
            id_heights: Default::default(),
            measured_once: false,
            folds: HashMap::new(),
            mode: ViewMode::LivePreview,
            tabstops: None,
            plugin_gen: 0,
            deco_cache: HashMap::new(),
            deco_retry: false,
            numbers_cache: ((u64::MAX, 0), Rc::default()),
            text_rc: (u64::MAX, Rc::from("")),
            hover: None,
            hover_pending: None,
            suggest: None,
            suggest_dismissed: None,
            suggest_key: (u64::MAX, 0),
            classes: (u64::MAX, Rc::default()),
            plain: false,
            perf: Perf::default(),
        };
        this.proj.version = u64::MAX;
        this.plain = !obsidilight_core::index::is_note(&this.path.to_string_lossy());
        this
    }

    // ---- external changes -------------------------------------------------

    /// The file was moved/renamed on disk (by the workspace).
    pub fn set_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = path;
        cx.notify();
    }

    /// Applies link-update edits computed by the workspace (byte ranges in the current text,
    /// ascending, non-overlapping) as one undoable step, then saves.
    pub fn apply_edits(&mut self, edits: &[(Range<usize>, String)], cx: &mut Context<Self>) {
        let sel = self.doc.selection();
        let mut delta: isize = 0;
        for (r, t) in edits {
            // ranges refer to the text before any of these edits; shift by what came earlier
            let start = (r.start as isize + delta) as usize;
            let end = (r.end as isize + delta) as usize;
            self.doc.replace(start..end, t, EditKind::Other);
            delta += t.len() as isize - (r.end - r.start) as isize;
        }
        self.doc.set_selection(Selection::caret(self.doc.clamp(sel.head)));
        self.save(cx);
        cx.notify();
    }

    /// Something outside this note changed (e.g. a linked file appeared): rebuild the projection.
    pub fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.proj.version = u64::MAX;
        cx.notify();
    }

    /// The same note changed in another pane (live sync): apply the difference as a remote
    /// change. Saving is left to the pane where the edit was made.
    pub fn apply_remote(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.doc.apply_remote(range, text);
        self.tabstops = None;
        cx.notify();
    }

    /// The workspace's vault watcher saw this file change.
    pub fn on_disk_changed(&mut self, cx: &mut Context<Self>) {
        let Ok(l) = file::load(&self.path) else { return };
        if Some(l.disk) == self.disk {
            return; // our own save, or no real change
        }
        if self.doc.len() == l.text.len() && self.doc.text() == l.text {
            // what is on disk is what we show (another pane of this note saved it)
            self.disk = Some(l.disk);
            self.doc.mark_saved();
            if self.status.as_ref().is_some_and(|s| s.starts_with("The file changed") || s.starts_with("Not saved")) {
                self.status = None;
            }
            cx.notify();
            return;
        }
        if self.doc.is_dirty() {
            self.status = Some("The file changed on disk while you have unsaved edits. Your edits are kept; saving will not overwrite the newer file.".into());
        } else {
            self.doc.reset(&l.text);
            self.disk = Some(l.disk);
            self.doc.mark_saved();
            self.status = None;
        }
        cx.notify();
    }

    // ---- saving -----------------------------------------------------------

    /// Benchmark mode: edits are never written to disk.
    pub fn disable_saving(&mut self) {
        self.saving_disabled = true;
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving_disabled || !self.doc.is_dirty() {
            return;
        }
        let text = self.doc.text();
        match file::save(&self.path, &text, self.disk) {
            Ok(SaveOutcome::Saved(d) | SaveOutcome::Unchanged(d)) => {
                self.disk = Some(d);
                self.doc.mark_saved();
                self.status = None;
                cx.emit(EditorEvent::Saved(self.path.clone()));
            }
            Ok(SaveOutcome::Conflict { .. }) => {
                self.status = Some("Not saved: the file on disk is newer than the version you opened. Your text is kept in the editor.".into());
            }
            Err(e) => self.status = Some(format!("Save failed: {e}").into()),
        }
        cx.notify();
    }

    /// Obsidian saves continuously; we save 1 s after the last edit.
    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_generation += 1;
        let generation = self.save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1000)).await;
            let _ = this.update(cx, |e, cx| {
                if e.save_generation == generation {
                    e.save(cx);
                }
            });
        })
        .detach();
    }

    // ---- editing primitives -----------------------------------------------

    pub fn edit(&mut self, range: Range<usize>, text: &str, kind: EditKind, cx: &mut Context<Self>) {
        if self.mode == ViewMode::Reading || self.status.as_ref().is_some_and(|s| s.starts_with("Cannot open")) {
            return;
        }
        if self.perf.pending_edit.is_none() {
            self.perf.pending_edit = Some(Instant::now());
        }
        api::apply_edit(&mut self.doc, &mut self.tabstops, range, text, kind);
        self.goal_x = None;
        self.autoscroll = true;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_selection(&mut self, sel: Selection, keep_goal: bool, cx: &mut Context<Self>) {
        self.doc.set_selection(sel);
        api::tabstops_on_selection(&mut self.tabstops, &self.doc.selection().range());
        if !keep_goal {
            self.goal_x = None;
        }
        self.marked = None;
        self.autoscroll = true;
        cx.notify();
    }

    fn move_to(&mut self, head: usize, select: bool, cx: &mut Context<Self>) {
        let s = self.doc.selection();
        let sel = if select { Selection::new(s.anchor, head) } else { Selection::caret(head) };
        self.set_selection(sel, false, cx);
    }

    fn line_ending(&self) -> &'static str {
        let t = self.doc.slice(0..self.doc.len().min(4096));
        if t.contains("\r\n") { "\r\n" } else { "\n" }
    }

    // ---- projection --------------------------------------------------------

    /// The note's `cssclasses` (from its frontmatter, re-read when the text changes).
    pub(crate) fn css_classes(&mut self) -> Rc<Vec<String>> {
        if self.classes.0 != self.doc.version() {
            let head = self.doc.slice(0..self.doc.len().min(8192));
            self.classes = (self.doc.version(), Rc::new(obsidilight_core::index::cssclasses(&head)));
        }
        self.classes.1.clone()
    }

    pub(crate) fn projection(&mut self, cx: &App) -> &Projection {
        // plugins, the theme (and light/dark) change what the projection draws
        let plugin_gen = cx.try_global::<PluginHost>().map_or(0, |h| h.generation) ^ (crate::theme::generation(cx) << 32) ^ (u64::from(cx.theme().is_dark()) << 63);
        if self.proj.version != self.doc.version() || self.proj.sel != self.doc.selection() || self.proj.mode != self.mode || self.plugin_gen != plugin_gen {
            self.plugin_gen = plugin_gen;
            if self.plain {
                self.proj = projection::project_plain(&self.doc.text(), self.doc.selection(), self.doc.version());
                return &self.proj;
            }
            let classes = self.css_classes();
            let highlight = crate::theme::text_style(cx, &classes).2;
            let t = Instant::now();
            let text = self.doc.text();
            let vault = self.vault.borrow();
            let path = &self.path;
            let resolve = |target: &str| vault.resolve(target, path);
            let host = cx.try_global::<PluginHost>();
            let code_mode = |lang: &str| host.map_or(CodeMode::Plain, |h| h.code_mode(lang));
            let labelers: Vec<Rc<dyn crate::plugins::LinkLabeler>> = host.map(|h| h.reg.link_labelers.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default();
            let this_rel = vault.relative(path);
            let link_label = |target: &str| -> Option<String> {
                if labelers.is_empty() {
                    return None;
                }
                let (note, sub) = target.split_once('#')?;
                let rel = if note.is_empty() { Some(this_rel.clone()) } else { vault.index.resolve(note, &this_rel) };
                let rel = rel?;
                let target_text = if rel == this_rel { text.as_str() } else { vault.index.text(&rel)? };
                let t = crate::plugins::NoteCtx { path: &rel, text: target_text, index: &vault.index };
                labelers.iter().find_map(|l| l.label(&t, sub))
            };
            self.proj = projection::project(&ProjectionInput {
                text: &text,
                sel: self.doc.selection(),
                mode: self.mode,
                version: self.doc.version(),
                palette: Palette { highlight },
                resolve: &resolve,
                link_label: &link_label,
                code_mode: &code_mode,
                key_base: 0,
            });
            self.perf.projection_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        &self.proj
    }

    // ---- actions ----------------------------------------------------------

    fn horiz(&mut self, dir: i32, word: bool, select: bool, cx: &mut Context<Self>) {
        let s = self.doc.selection();
        if !select && !s.is_empty() && !word {
            let r = s.range();
            return self.move_to(if dir < 0 { r.start } else { r.end }, false, cx);
        }
        let h = s.head;
        let to = match (dir < 0, word) {
            (true, false) => self.doc.prev_grapheme(h),
            (false, false) => self.doc.next_grapheme(h),
            (true, true) => self.doc.prev_word(h),
            (false, true) => self.doc.next_word(h),
        };
        self.move_to(to, select, cx);
    }

    fn line_edge(&mut self, end: bool, select: bool, cx: &mut Context<Self>) {
        let line = self.doc.line_of(self.doc.selection().head);
        let to = if end { self.doc.line_end(line) } else { self.doc.line_start(line) };
        self.move_to(to, select, cx);
    }

    fn delete(&mut self, dir: i32, word: bool, cx: &mut Context<Self>) {
        let s = self.doc.selection();
        let r = if !s.is_empty() {
            s.range()
        } else if dir < 0 {
            (if word { self.doc.prev_word(s.head) } else { self.doc.prev_grapheme(s.head) })..s.head
        } else {
            s.head..(if word { self.doc.next_word(s.head) } else { self.doc.next_grapheme(s.head) })
        };
        if !r.is_empty() {
            self.edit(r, "", EditKind::Delete, cx);
        }
    }

    /// Enter: new line, continuing a list item's marker (or ending the list on an empty item).
    fn newline(&mut self, cx: &mut Context<Self>) {
        let nl = self.line_ending();
        let s = self.doc.selection();
        let line = self.doc.line_of(s.head);
        let text = self.doc.line_text(line);
        let ls = self.doc.line_start(line);
        let indent: String = text.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let rest = &text[indent.len()..];
        // (marker on this line, marker for the next line)
        let markers: Option<(String, String)> = ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "]
            .iter()
            .find(|m| rest.starts_with(**m))
            .map(|m| (m.to_string(), m.replace("[x]", "[ ]").replace("[X]", "[ ]")))
            .or_else(|| {
                let d: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let n: u32 = d.parse().ok()?;
                let sep = rest[d.len()..].chars().next().filter(|c| *c == '.' || *c == ')')?;
                rest[d.len() + 1..].starts_with(' ').then(|| (format!("{n}{sep} "), format!("{}{sep} ", n + 1)))
            });
        match markers {
            // empty item: Enter ends the list instead of continuing it
            Some((cur, _)) if s.is_empty() && rest.trim_end() == cur.trim_end() => self.edit(ls..ls + text.len(), "", EditKind::Other, cx),
            Some((_, next)) => self.edit(s.range(), &format!("{nl}{indent}{next}"), EditKind::Other, cx),
            None => self.edit(s.range(), nl, EditKind::Insert, cx),
        }
    }

    fn copy_text(&self) -> Option<String> {
        let s = self.doc.selection();
        (!s.is_empty()).then(|| self.doc.slice(s.range()))
    }

    pub fn paste_item(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        for entry in item.entries() {
            if let ClipboardEntry::Image(img) = entry {
                let saved = self.vault.borrow_mut().save_pasted_image(&img.bytes, img.format == ImageFormat::Png);
                match saved {
                    Ok(name) => {
                        let r = self.doc.selection().range();
                        self.edit(r, &format!("![[{name}]]"), EditKind::Other, cx);
                    }
                    Err(e) => self.status = Some(format!("Could not save pasted image: {e}").into()),
                }
                return;
            }
        }
        if let Some(text) = item.text() {
            let r = self.doc.selection().range();
            self.edit(r, &text, EditKind::Other, cx);
        }
    }

    /// Ctrl+click on a link: external URLs open in the browser, wikilinks open the note.
    pub fn open_link(&mut self, target: &str, new_tab: bool, cx: &mut Context<Self>) {
        if target.starts_with("tag:") || target.is_empty() {
            return;
        }
        let script = std::env::var_os("OBSIDILIGHT_SCRIPT").is_some();
        if target.contains("://") || target.starts_with("mailto:") {
            println!("OPEN_URL {target}");
            if !script {
                cx.open_url(target);
            }
            return;
        }
        cx.emit(EditorEvent::OpenLink { target: target.to_string(), new_tab });
        cx.notify();
    }

    /// Ticks or unticks the task whose `[ ]` / `[x]` is at `at` (clicking its checkbox; works in
    /// reading view too, as in Obsidian). One undoable edit, saved like typing.
    pub fn toggle_task(&mut self, at: Range<usize>, cx: &mut Context<Self>) {
        let now = match self.doc.slice(at.clone()).as_str() {
            "[ ]" => "[x]",
            "[x]" | "[X]" => "[ ]",
            _ => return, // the text moved under us: do nothing
        };
        if self.status.as_ref().is_some_and(|s| s.starts_with("Cannot open")) {
            return;
        }
        let sel = self.doc.selection();
        api::apply_edit(&mut self.doc, &mut self.tabstops, at, now, EditKind::Other);
        self.doc.set_selection(sel);
        if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
            println!("TASK {now}");
        }
        self.schedule_save(cx);
        cx.notify();
    }

    /// Switches to `mode`, or back to live preview if already in it (Ctrl+E, Ctrl+Shift+E).
    pub fn toggle_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.mode = if self.mode == mode { ViewMode::LivePreview } else { mode };
        self.autoscroll = true;
        cx.notify();
    }

    /// Moves the caret to `#Heading` or `#^block-id` (the part after `#` in a link).
    pub fn go_to_subpath(&mut self, sub: &str, cx: &mut Context<Self>) {
        let text = self.doc.text();
        let found = if let Some(id) = sub.strip_prefix('^') {
            let pat = format!("^{id}");
            let mut off = 0;
            let mut hit = None;
            for line in text.split('\n') {
                let t = line.trim_end();
                if t.ends_with(&pat) && t[..t.len() - pat.len()].chars().next_back().is_none_or(|c| c.is_whitespace() || c == '>') {
                    hit = Some(off);
                    break;
                }
                off += line.len() + 1;
            }
            hit
        } else {
            // `[[note#A#B]]` targets heading B (under A)
            let want = sub.rsplit('#').next().unwrap_or(sub).trim().to_lowercase();
            obsidilight_core::markdown::parse_blocks(&text)
                .into_iter()
                .find(|b| matches!(b.kind, obsidilight_core::markdown::BlockKind::Heading { .. }) && text[b.content.clone()].trim().to_lowercase() == want)
                .map(|b| b.range.start)
        };
        match found {
            Some(off) => self.set_selection(Selection::caret(off), false, cx),
            None => self.status = Some(format!("\"{sub}\" not found in this note").into()),
        }
        cx.notify();
    }

    /// Drag-and-drop: images are copied into the attachment folder and embedded at the drop point.
    fn drop_files(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.mouse_position();
        if let Some(off) = self.hit_test(at, input::Bias::Nearest) {
            self.doc.set_selection(Selection::caret(off));
        }
        let mut embeds = Vec::new();
        for p in paths.paths() {
            if crate::embeds::is_image(p) {
                let imported = self.vault.borrow_mut().import_file(p);
                match imported {
                    Ok(name) => embeds.push(format!("![[{name}]]")),
                    Err(e) => self.status = Some(format!("Could not import {}: {e}", p.display()).into()),
                }
            }
        }
        if !embeds.is_empty() {
            let r = self.doc.selection().range();
            self.edit(r, &embeds.join("\n"), EditKind::Other, cx);
        }
    }

    fn register_actions(div: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        div.on_action(cx.listener(|e, _: &MoveLeft, _, cx| e.horiz(-1, false, false, cx)))
            .on_action(cx.listener(|e, _: &MoveRight, _, cx| e.horiz(1, false, false, cx)))
            .on_action(cx.listener(|e, _: &MoveWordLeft, _, cx| e.horiz(-1, true, false, cx)))
            .on_action(cx.listener(|e, _: &MoveWordRight, _, cx| e.horiz(1, true, false, cx)))
            .on_action(cx.listener(|e, _: &SelectLeft, _, cx| e.horiz(-1, false, true, cx)))
            .on_action(cx.listener(|e, _: &SelectRight, _, cx| e.horiz(1, false, true, cx)))
            .on_action(cx.listener(|e, _: &SelectWordLeft, _, cx| e.horiz(-1, true, true, cx)))
            .on_action(cx.listener(|e, _: &SelectWordRight, _, cx| e.horiz(1, true, true, cx)))
            .on_action(cx.listener(|e, _: &MoveUp, _, cx| {
                if !e.suggest_move(-1, cx) {
                    e.vertical(-1, false, false, cx)
                }
            }))
            .on_action(cx.listener(|e, _: &MoveDown, _, cx| {
                if !e.suggest_move(1, cx) {
                    e.vertical(1, false, false, cx)
                }
            }))
            .on_action(cx.listener(|e, _: &Cancel, _, cx| {
                if let Some(s) = e.suggest.take() {
                    e.suggest_dismissed = Some(s.start);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|e, _: &SelectUp, _, cx| e.vertical(-1, true, false, cx)))
            .on_action(cx.listener(|e, _: &SelectDown, _, cx| e.vertical(1, true, false, cx)))
            .on_action(cx.listener(|e, _: &PageUp, _, cx| e.vertical(-1, false, true, cx)))
            .on_action(cx.listener(|e, _: &PageDown, _, cx| e.vertical(1, false, true, cx)))
            .on_action(cx.listener(|e, _: &MoveLineStart, _, cx| e.line_edge(false, false, cx)))
            .on_action(cx.listener(|e, _: &MoveLineEnd, _, cx| e.line_edge(true, false, cx)))
            .on_action(cx.listener(|e, _: &SelectLineStart, _, cx| e.line_edge(false, true, cx)))
            .on_action(cx.listener(|e, _: &SelectLineEnd, _, cx| e.line_edge(true, true, cx)))
            .on_action(cx.listener(|e, _: &MoveDocStart, _, cx| e.move_to(0, false, cx)))
            .on_action(cx.listener(|e, _: &MoveDocEnd, _, cx| e.move_to(e.doc.len(), false, cx)))
            .on_action(cx.listener(|e, _: &SelectDocStart, _, cx| e.move_to(0, true, cx)))
            .on_action(cx.listener(|e, _: &SelectDocEnd, _, cx| e.move_to(e.doc.len(), true, cx)))
            .on_action(cx.listener(|e, _: &SelectAll, _, cx| e.set_selection(Selection::new(0, e.doc.len()), false, cx)))
            .on_action(cx.listener(|e, _: &Backspace, _, cx| {
                if !e.run_key_hooks(EditorKey::Backspace, cx) {
                    // inside an empty auto-paired (), [], "" … both halves go
                    let s = e.doc.selection();
                    let (b, a) = (e.char_before(s.head), e.char_after(s.head));
                    if s.is_empty() && crate::app_settings::AppSettings::flag(cx, "autoPairBrackets") && assist::deletes_pair(b, a) {
                        let n = b.map_or(1, char::len_utf8) + a.map_or(1, char::len_utf8);
                        let start = s.head - b.map_or(1, char::len_utf8);
                        e.edit(start..start + n, "", EditKind::Delete, cx);
                    } else {
                        e.delete(-1, false, cx)
                    }
                }
            }))
            .on_action(cx.listener(|e, _: &Delete, _, cx| e.delete(1, false, cx)))
            .on_action(cx.listener(|e, _: &DeleteWordLeft, _, cx| e.delete(-1, true, cx)))
            .on_action(cx.listener(|e, _: &DeleteWordRight, _, cx| e.delete(1, true, cx)))
            .on_action(cx.listener(|e, _: &Newline, _, cx| {
                if e.suggest_accept(None, cx) {
                    return;
                }
                if !e.run_key_hooks(EditorKey::Enter, cx) {
                    e.newline(cx)
                }
            }))
            .on_action(cx.listener(|e, _: &ShiftNewline, _, cx| {
                if !e.run_key_hooks(EditorKey::ShiftEnter, cx) {
                    e.newline(cx)
                }
            }))
            .on_action(cx.listener(|e, _: &Tab, _, cx| {
                if e.suggest_accept(None, cx) {
                    return;
                }
                if !e.run_key_hooks(EditorKey::Tab, cx) {
                    let r = e.doc.selection().range();
                    e.edit(r, "\t", EditKind::Insert, cx)
                }
            }))
            .on_action(cx.listener(|e, _: &ShiftTab, _, cx| {
                e.run_key_hooks(EditorKey::ShiftTab, cx);
            }))
            .on_action(cx.listener(|e, _: &Undo, _, cx| {
                if e.mode != ViewMode::Reading && e.doc.undo() {
                    e.tabstops = None;
                    e.autoscroll = true;
                    e.schedule_save(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|e, _: &Redo, _, cx| {
                if e.mode != ViewMode::Reading && e.doc.redo() {
                    e.tabstops = None;
                    e.autoscroll = true;
                    e.schedule_save(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|e, _: &Copy, _, cx| {
                if let Some(t) = e.copy_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                }
            }))
            .on_action(cx.listener(|e, _: &Cut, _, cx| {
                if let Some(t) = e.copy_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                    let r = e.doc.selection().range();
                    e.edit(r, "", EditKind::Other, cx);
                }
            }))
            .on_action(cx.listener(|e, _: &Paste, _, cx| {
                if let Some(item) = cx.read_from_clipboard() {
                    e.paste_item(item, cx);
                }
            }))
            .on_action(cx.listener(|e, _: &Save, _, cx| e.save(cx)))
            .on_action(cx.listener(|e, _: &ToggleReading, _, cx| e.toggle_mode(ViewMode::Reading, cx)))
            .on_action(cx.listener(|e, _: &ToggleSource, _, cx| e.toggle_mode(ViewMode::Source, cx)))
    }
}

/// The live editor as an [`api::Backend`] for plugin hooks.
struct Live<'a, 'b> {
    e: &'a mut Editor,
    cx: &'a mut Context<'b, Editor>,
}

impl api::Backend for Live<'_, '_> {
    fn doc(&self) -> &Document {
        &self.e.doc
    }
    fn tabstops(&mut self) -> &mut Option<TabstopSession> {
        &mut self.e.tabstops
    }
    fn edit(&mut self, r: Range<usize>, text: &str, kind: EditKind) {
        self.e.edit(r, text, kind, self.cx);
    }
    fn select(&mut self, sel: Selection) {
        self.e.set_selection(sel, false, self.cx);
    }
    fn path(&self) -> &std::path::Path {
        &self.e.path
    }
}

pub(crate) struct HoverPreview {
    link: String,
    at: Point<Pixels>,
    snippet: String,
    numbers: HashMap<usize, String>,
}

/// The open link-suggestion popup.
pub(crate) struct SuggestState {
    /// Document offset just after `[[`.
    pub start: usize,
    /// The caret (end of the query).
    pub end: usize,
    pub items: Vec<assist::Suggestion>,
    pub selected: usize,
}

impl Editor {
    /// The internal link under the mouse: its target (`note#sub`), if a plugin's hover policy
    /// previews it now (page preview: with Ctrl; Academic Notes: block references on hover).
    fn link_under(&self, p: Point<Pixels>, ctrl: bool, cx: &App) -> Option<String> {
        let off = self.char_under(p)?;
        let line = self.doc.line_of(off);
        let ls = self.doc.line_start(line);
        let text = self.doc.line_text(line);
        let col = off - ls;
        let n = obsidilight_core::markdown::parse_inline(&text, 0..text.len())
            .into_iter()
            .find(|n| matches!(n.kind, obsidilight_core::markdown::InlineKind::WikiLink { embed: false }) && n.range.start <= col && col < n.range.end)?;
        let target = n.link_target(&text).to_string();
        let source = if self.mode == ViewMode::Reading { crate::plugins::HoverSource::Reading } else { crate::plugins::HoverSource::Editor };
        PluginHost::wants_preview(cx, &target, source, ctrl).then_some(target)
    }

    pub(crate) fn on_hover(&mut self, p: Point<Pixels>, ctrl: bool, cx: &mut Context<Self>) {
        let target = if self.dragging { None } else { self.link_under(p, ctrl, cx) };
        if target.is_some() && self.hover.as_ref().map(|h| &h.link) == target.as_ref() {
            return;
        }
        if target.is_none() {
            self.hover_pending = None;
            if self.hover.take().is_some() {
                cx.notify();
            }
            return;
        }
        if self.hover_pending == target {
            return;
        }
        self.hover = None;
        self.hover_pending = target.clone();
        cx.spawn(async move |e, cx| {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let _ = e.update(cx, |e, cx| {
                if e.hover_pending == target {
                    e.hover_pending = None;
                    if let Some(t) = target {
                        e.hover = e.build_preview(&t, p, cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// For UI tests.
    pub(crate) fn debug_numbers(&mut self, cx: &App) -> HashMap<usize, String> {
        (*self.callout_numbers(cx)).clone()
    }
    pub(crate) fn debug_hover(&self) -> Option<String> {
        self.hover.as_ref().map(|h| h.snippet.lines().next().unwrap_or("").to_string())
    }

    fn build_preview(&self, target: &str, at: Point<Pixels>, cx: &App) -> Option<HoverPreview> {
        let (note, sub) = target.split_once('#').unwrap_or((target, ""));
        let text = if note.is_empty() {
            self.doc.text()
        } else {
            let p = self.vault.borrow().resolve(note, &self.path)?;
            if p == self.path { self.doc.text() } else { std::fs::read_to_string(p).ok()? }
        };
        let (snippet, first) = preview::snippet(&text, sub)?;
        let numbers = {
            let vault = self.vault.borrow();
            let rel = if note.is_empty() { vault.relative(&self.path) } else { vault.resolve(note, &self.path).map(|p| vault.relative(&p)).unwrap_or_default() };
            preview::numbers(&rel, &text, first, &vault.index, cx)
        };
        Some(HoverPreview { link: target.to_string(), at, snippet, numbers })
    }

    fn render_hover(&self, md: &render::MarkdownRenderer, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let h = self.hover.as_ref()?;
        Some(preview::card(md, &h.snippet, h.numbers.clone(), h.at, window, cx))
    }

    /// Callout labels from plugins (numbering), recomputed when the text or plugins change.
    fn callout_numbers(&mut self, cx: &App) -> Rc<HashMap<usize, String>> {
        let host = cx.try_global::<PluginHost>();
        // other notes matter too (which equations they reference): the index generation
        let index_gen = self.vault.borrow().index.generation();
        let key = (self.doc.version(), host.map_or(0, |h| h.generation).wrapping_add(index_gen.wrapping_mul(1 << 20)));
        if self.numbers_cache.0 != key {
            let renderers: Vec<_> = host.map(|h| h.reg.callout_renderers.iter().map(|(_, r)| r.clone()).collect()).unwrap_or_default();
            let mut map = HashMap::new();
            if !renderers.is_empty() {
                let text = self.doc.text();
                let vault = self.vault.borrow();
                let rel = vault.relative(&self.path);
                let ctx = crate::plugins::NoteCtx { path: &rel, text: &text, index: &vault.index };
                for r in renderers {
                    for (k, v) in r.numbers(&ctx) {
                        map.entry(k).or_insert(v);
                    }
                }
            }
            self.numbers_cache = (key, Rc::new(map));
        }
        self.numbers_cache.1.clone()
    }

    pub(crate) fn char_before(&self, at: usize) -> Option<char> {
        (at > 0).then(|| self.doc.slice(at.saturating_sub(4)..at).chars().next_back()).flatten()
    }
    pub(crate) fn char_after(&self, at: usize) -> Option<char> {
        self.doc.slice(at..(at + 4).min(self.doc.len())).chars().next()
    }

    /// Recomputes link suggestions when the text or caret moved.
    fn refresh_suggest(&mut self) {
        let sel = self.doc.selection();
        let key = (self.doc.version(), sel.head);
        if key == self.suggest_key {
            return;
        }
        self.suggest_key = key;
        let keep = self.suggest.as_ref().map_or(0, |s| s.selected);
        self.suggest = None;
        if !sel.is_empty() || self.mode == ViewMode::Reading || self.plain {
            return;
        }
        let line = self.doc.line_of(sel.head);
        let ls = self.doc.line_start(line);
        let text = self.doc.line_text(line);
        let Some((qs, query, _embed)) = assist::link_query(&text, sel.head - ls) else {
            self.suggest_dismissed = None;
            return;
        };
        let start = ls + qs;
        if self.suggest_dismissed == Some(start) {
            return;
        }
        let vault = self.vault.borrow();
        let from = vault.relative(&self.path);
        let items = assist::suggestions(&vault.index, &from, &query, 50);
        if !items.is_empty() {
            let selected = keep.min(items.len() - 1);
            self.suggest = Some(SuggestState { start, end: sel.head, items, selected: if query.is_empty() { 0 } else { selected } });
        }
    }

    fn suggest_move(&mut self, d: i32, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.suggest.as_mut() else { return false };
        let n = s.items.len() as i32;
        s.selected = ((s.selected as i32 + d).rem_euclid(n)) as usize;
        cx.notify();
        true
    }

    /// Inserts the chosen suggestion (`ix`, or the highlighted one) and closes the link.
    pub(crate) fn suggest_accept(&mut self, ix: Option<usize>, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.suggest.take() else { return false };
        let Some(item) = s.items.get(ix.unwrap_or(s.selected)) else { return false };
        let has_close = self.doc.slice(s.end..(s.end + 2).min(self.doc.len())) == "]]";
        let text = if has_close { item.insert.clone() } else { format!("{}]]", item.insert) };
        self.edit(s.start..s.end, &text, EditKind::Other, cx);
        let caret = s.start + item.insert.len() + 2;
        self.set_selection(Selection::caret(caret), false, cx);
        self.suggest_dismissed = Some(s.start);
        true
    }

    /// The popup under the caret.
    fn render_suggest(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.refresh_suggest();
        let s = self.suggest.as_ref()?;
        let caret = self.caret_bounds(self.doc.selection().head)?;
        let theme = cx.theme().clone();
        const SHOWN: usize = 10;
        let top = s.selected.saturating_sub(SHOWN - 1);
        let rows: Vec<AnyElement> = s
            .items
            .iter()
            .enumerate()
            .skip(top)
            .take(SHOWN)
            .map(|(i, it)| {
                div()
                    .id(("link-suggestion", i))
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(i == s.selected, |d| d.bg(theme.accent))
                    .child(div().text_sm().child(it.label.clone()))
                    .when(!it.detail.is_empty(), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(it.detail.clone())))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |e, _: &MouseDownEvent, _, cx| {
                            e.suggest_accept(Some(i), cx);
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        let popup = div()
            .w(crate::ui::r(380.))
            .p_1()
            .flex()
            .flex_col()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(px(6.))
            .shadow_lg()
            .children(rows)
            .when(s.items.len() > SHOWN, |d| d.child(div().px_2().text_xs().text_color(theme.muted_foreground).child(format!("{}{} matches · ↑↓ to browse", s.items.len(), if s.items.len() >= 50 { "+" } else { "" }))));
        Some(deferred(anchored().position(point(caret.left(), caret.bottom() + px(4.))).snap_to_window_with_margin(px(8.)).child(popup)).with_priority(1).into_any_element())
    }

    /// Plugin decorations (e.g. misspellings) for the rendered blocks, as leaf ranges. Results
    /// are cached per block text, so typing re-checks only the edited block. The word at the
    /// caret is left alone while it is being typed.
    fn decorations(&mut self, blocks: Range<usize>, cx: &mut Context<Self>) -> Vec<(u64, Range<usize>, Hsla)> {
        use std::hash::{Hash, Hasher};
        let decorators: Vec<Rc<dyn crate::plugins::TextDecorator>> = cx.try_global::<PluginHost>().map(|h| h.reg.decorators.iter().map(|(_, d)| d.clone()).collect()).unwrap_or_default();
        if decorators.is_empty() || self.mode == ViewMode::Reading || self.plain {
            return Vec::new();
        }
        if self.deco_cache.len() > 4000 {
            self.deco_cache.clear();
        }
        let head = self.doc.selection().head;
        let mut out = Vec::new();
        let mut pending = false;
        for ix in blocks {
            let b = &self.proj.blocks[ix];
            if b.keys.is_empty() || matches!(b.node, projection::ProjNode::Block(gpui_kit::base::text::editor_host::BlockNode::CodeBlock(_))) {
                continue;
            }
            let leaves: Vec<(u64, &projection::LeafMap)> = b.keys.iter().filter_map(|k| self.proj.leaf(*k).map(|m| (*k, m))).collect();
            let Some(start) = leaves.iter().map(|(_, m)| m.src.start).min() else { continue };
            let end = leaves.iter().map(|(_, m)| m.src.end).max().unwrap_or(start);
            let text = self.doc.slice(start..end);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut h);
            let th = h.finish();
            for (di, d) in decorators.iter().enumerate() {
                if !d.ready() {
                    pending = true;
                    continue;
                }
                let ranges = self.deco_cache.entry((th, di, d.revision())).or_insert_with(|| Rc::new(d.decorate(&text))).clone();
                let color = d.color();
                for r in ranges.iter() {
                    let abs = start + r.start..start + r.end;
                    if abs.start <= head && head <= abs.end {
                        continue;
                    }
                    for (k, m) in &leaves {
                        if let Some(lr) = m.leaf_range(&abs) {
                            out.push((*k, lr, color));
                        }
                    }
                }
            }
        }
        if pending && !self.deco_retry {
            self.deco_retry = true;
            cx.spawn(async move |e, cx| {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let _ = e.update(cx, |e, cx| {
                    e.deco_retry = false;
                    cx.notify();
                });
            })
            .detach();
        }
        out
    }

    fn hooks(&self, cx: &App) -> Vec<Rc<dyn EditorHook>> {
        if self.plain {
            return Vec::new(); // Markdown behaviour (snippets, pairs in math) is for notes
        }
        cx.try_global::<PluginHost>().map(|h| h.reg.editor_hooks.iter().map(|(_, h)| h.clone()).collect()).unwrap_or_default()
    }

    fn editable(&self) -> bool {
        self.mode != ViewMode::Reading && !self.status.as_ref().is_some_and(|s| s.starts_with("Cannot open"))
    }

    /// Lets plugin hooks handle a key first; true when one consumed it.
    pub(crate) fn run_key_hooks(&mut self, key: EditorKey, cx: &mut Context<Self>) -> bool {
        if !self.editable() {
            return false;
        }
        for h in self.hooks(cx) {
            let mut live = Live { e: self, cx };
            if h.on_key(&mut EditorApi::new(&mut live), key) {
                return true;
            }
        }
        false
    }

    /// Lets plugin hooks handle a typed character before it is inserted.
    pub(crate) fn run_char_hooks(&mut self, ch: &str, cx: &mut Context<Self>) -> bool {
        if !self.editable() {
            return false;
        }
        for h in self.hooks(cx) {
            let mut live = Live { e: self, cx };
            if h.on_char(&mut EditorApi::new(&mut live), ch) {
                return true;
            }
        }
        false
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.perf.render_start = Some(Instant::now());
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let sel_color = theme.selection;
        let sel_range = self.doc.selection().range();
        let extensions = self.extensions.clone();
        self.projection(cx);
        let classes = self.css_classes();
        let numbers = self.callout_numbers(cx);
        let proj = &self.proj;
        // Links open on Ctrl+click only; a plain click places the caret (handled by on_mouse_down).
        // Without a handler gpui-kit would open every link on a plain click.
        let weak = cx.entity().downgrade();
        let (mut style, body_size, _) = crate::theme::text_style(cx, &classes);
        if self.plain {
            // plain text, not a code block: no box, no padding
            let mut r = StyleRefinement::default();
            r.background = Some(gpui_kit::transparent_black().into());
            r.padding = EdgesRefinement { top: Some(px(0.).into()), right: Some(px(0.).into()), bottom: Some(px(0.).into()), left: Some(px(0.).into()) };
            r.border_widths = EdgesRefinement { top: Some(px(0.).into()), right: Some(px(0.).into()), bottom: Some(px(0.).into()), left: Some(px(0.).into()) };
            style = style.with_code_block(r);
        }
        let mut rcx = RenderContext::from_app(cx)
            .with_style(style)
            .with_markdown_extensions(extensions)
            .with_body_font_size(px(body_size))
            .on_link_click(move |url, ev, _, cx| {
                if ev.modifiers().secondary(cx) {
                    let url = url.to_string();
                    let new_tab = ev.modifiers().shift;
                    let _ = weak.update(cx, |e, cx| e.open_link(&url, new_tab, cx));
                }
            });
        // one call per leaf: own colours first, then muted markers (later ranges win)
        let mut fg: HashMap<u64, Vec<(Range<usize>, Hsla)>> = proj.colors.clone();
        for (k, ranges) in &proj.muted {
            fg.entry(*k).or_default().extend(ranges.iter().map(|r| (r.clone(), muted)));
        }
        for (k, ranges) in fg {
            rcx = rcx.with_foreground_ranges(k, ranges);
        }
        let decorations: HashMap<u64, Vec<(Range<usize>, Hsla)>> =
            proj.selection_ranges(&sel_range).into_iter().map(|(k, v)| (k, v.into_iter().map(|r| (r, sel_color)).collect())).collect();
        let n = proj.blocks.len();
        // ---- virtualization: render the blocks within a screen of the viewport (plus the caret's)
        let caret_key = proj.locate(self.doc.selection().head).map(|(k, _)| k);
        let caret_ix = caret_key.and_then(|k| proj.blocks.iter().position(|b| b.keys.contains(&k)));
        let view_h = f32::from(self.scroll.bounds().size.height);
        let top = -f32::from(self.scroll.offset().y);
        let heights = self.heights.borrow();
        let id_heights = self.id_heights.borrow();
        const EST: f32 = 30.0;
        let h = |ix: usize| heights.get(&proj.blocks[ix].hash).copied().or_else(|| id_heights.get(&proj.blocks[ix].id).copied()).unwrap_or(EST);
        let all_known = proj.blocks.iter().all(|b| heights.contains_key(&b.hash) || id_heights.contains_key(&b.id));
        let mut ys = Vec::with_capacity(n + 1);
        let scale = crate::theme::ui_scale(cx);
        let pad = 24.0 * scale; // content top padding (py_6: 1.5 rem)
        let mut y = pad;
        for ix in 0..n {
            ys.push(y);
            y += h(ix);
        }
        ys.push(y);
        // the scroll offset may still be unclamped (a big wheel delta): clamp to the content,
        // which ends half a screen after the last block (scroll past end)
        let top = top.min((y + pad + view_h * 0.5 - view_h).max(0.0));
        let (lo, hi) = if view_h <= 0.0 || (!all_known && !self.measured_once) {
            (0, n) // first frame: lay everything out once to learn the heights
        } else {
            let first = ys.partition_point(|&y0| y0 < top - view_h).saturating_sub(1);
            let last = ys.partition_point(|&y0| y0 < top + 2.0 * view_h).min(n);
            match caret_ix {
                Some(c) if self.autoscroll && (c < first || c >= last) => (c.saturating_sub(10), (c + 10).min(n)),
                _ => (first, last.max(first)),
            }
        };
        drop(heights);
        drop(id_heights);
        self.measured_once |= lo == 0 && hi == n;
        let md = {
            let vault = self.vault.clone();
            let path = self.path.clone();
            let rel = self.vault.borrow().relative(&self.path);
            if self.text_rc.0 != self.doc.version() {
                self.text_rc = (self.doc.version(), self.doc.text().into());
            }
            render::MarkdownRenderer::new(Rc::new(move |t: &str| vault.borrow().resolve(t, &path))).with_note(rel, self.text_rc.1.clone())
        };
        let host = render::Host { editor: Some(cx.entity().downgrade()), folds: &self.folds, md: &md, numbers: Some(&numbers), classes: &classes };
        // this pane's text width (last frame): images wider than it shrink to fit
        let width = f32::from(self.scroll.bounds().size.width) - 64.0 * scale;
        if width > 0.0 {
            crate::atoms::CONTENT_WIDTH.with(|c| c.set(width));
        }
        let mut children: Vec<AnyElement> = Vec::with_capacity(hi - lo + 2);
        if lo > 0 {
            children.push(div().h(px(ys[lo] - pad)).into_any_element());
        }
        for ix in lo..hi {
            let b = &proj.blocks[ix];
            let el = render::render_node(&b.node, ix, &host, &rcx, window, cx);
            let el = if b.indent > 0.0 { div().pl(crate::ui::r(b.indent)).child(el).into_any_element() } else { el };
            let el = match b.align {
                // the block itself moves (not the glyphs), so caret and selection geometry follow
                Some(projection::BlockAlign::Center) => div().w_full().flex().justify_center().child(div().max_w_full().text_center().child(el)).into_any_element(),
                Some(projection::BlockAlign::Right) => div().w_full().flex().justify_end().child(div().max_w_full().text_right().child(el)).into_any_element(),
                _ => el, // justify: GPUI has no justified text; left as written
            };
            children.push(measured::Measured::new(b.hash, b.id, self.heights.clone(), self.id_heights.clone(), el).into_any_element());
        }
        if hi < n {
            children.push(div().h(px(ys[n] - ys[hi])).into_any_element());
        }
        // scroll past the end (like Obsidian): room below the last line to type and paste
        children.push(div().h(px((view_h * 0.5).max(0.0))).into_any_element());
        let squiggles = self.decorations(lo..hi, cx);
        let content = div().w_full().max_w(crate::ui::r(760.)).mx_auto().px_8().py_6().flex().flex_col().children(children);
        let suggest_popup = self.render_suggest(cx);
        let hover_popup = self.render_hover(&md, window, cx);
        let editor = cx.entity();
        let menu_editor = editor.downgrade();
        let status = self.status.clone();
        let root = div().id("editor").key_context(CONTEXT).track_focus(&self.focus).size_full().flex().flex_col().bg(theme.background).text_color(theme.foreground);
        self.perf.render_end = Some(Instant::now());
        Self::register_actions(root, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(input::on_mouse_down))
            .on_mouse_move(cx.listener(input::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(input::on_mouse_up))
            .on_drop(cx.listener(|e, paths: &ExternalPaths, window, cx| e.drop_files(paths, window, cx)))
            .when_some(status, |d, s| d.child(div().px_4().py_1().bg(theme.warning.opacity(0.2)).text_sm().child(s)))
            .when(self.mode != ViewMode::LivePreview, |d| {
                d.child(
                    div()
                        .px_4()
                        .py_0p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if self.mode == ViewMode::Reading { "Reading view (Ctrl+E to edit)" } else { "Source mode (Ctrl+Shift+E for live preview)" }),
                )
            })
            .child(
                div()
                    .id("editor-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(EditorElement::new(editor, content.into_any_element(), decorations, squiggles, self.scroll.clone())),
            )
            .children(suggest_popup)
            .children(hover_popup)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|e, ev: &MouseDownEvent, window, cx| {
                    // right-click outside the selection moves the caret there (spelling menu)
                    window.focus(&e.focus, cx);
                    if let Some(off) = e.hit_test(ev.position, input::Bias::Nearest) {
                        let r = e.doc.selection().range();
                        if !(r.start <= off && off <= r.end && !r.is_empty()) {
                            e.set_selection(Selection::caret(off), false, cx);
                        }
                    }
                }),
            )
            .context_menu(move |menu, _, cx| {
                let mut menu = menu;
                let entries: Vec<crate::plugins::MenuEntry> = match (menu_editor.upgrade(), cx.try_global::<PluginHost>()) {
                    (Some(ed), Some(h)) => {
                        let providers: Vec<_> = h.reg.editor_menus.iter().map(|(_, p)| p.clone()).collect();
                        let e = ed.read(cx);
                        providers.iter().flat_map(|p| p(e, cx)).collect()
                    }
                    _ => Vec::new(),
                };
                for it in &entries {
                    let weak = menu_editor.clone();
                    let run = it.run.clone();
                    menu = menu.item(gpui_kit::component::menu::PopupMenuItem::new(it.label.clone()).on_click(move |_, window, cx| {
                        let mut ctx = crate::plugins::ActionCtx { window, cx, editor: weak.upgrade() };
                        run(&mut ctx);
                    }));
                }
                if !entries.is_empty() {
                    menu = menu.separator();
                }
                let mut menu = menu.menu("Cut", Box::new(Cut)).menu("Copy", Box::new(Copy)).menu("Paste", Box::new(Paste));
                let items: Vec<_> = cx
                    .try_global::<PluginHost>()
                    .map(|h| h.reg.menu_items.iter().filter(|(_, m)| m.target == crate::plugins::MenuTarget::Editor).map(|(_, m)| m.clone()).collect())
                    .unwrap_or_default();
                if !items.is_empty() {
                    menu = menu.separator();
                }
                for it in items {
                    let weak = menu_editor.clone();
                    menu = menu.item(gpui_kit::component::menu::PopupMenuItem::new(it.label.clone()).on_click(move |_, window, cx| {
                        let mut ctx = crate::plugins::ActionCtx { window, cx, editor: weak.upgrade() };
                        (it.run)(&mut ctx, None);
                    }));
                }
                menu
            })
    }
}
