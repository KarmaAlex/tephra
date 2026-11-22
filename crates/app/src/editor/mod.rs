//! The live-preview Markdown editor view.

pub mod element;
pub mod input;
pub mod measured;
pub mod projection;

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::base::text::editor_host::{FlowPosition, LeafGeometry, RenderContext};
use gpui_kit::base::text::MarkdownExtensions;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::file::{self, DiskState, SaveOutcome};
use obsidilight_core::{Document, EditKind, Selection};

use crate::embeds::SharedVault;
use element::EditorElement;
use projection::{Palette, Projection, ProjectionInput};

actions!(
    editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, MoveWordLeft, MoveWordRight, MoveLineStart, MoveLineEnd, MoveDocStart, MoveDocEnd,
        SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight, SelectLineStart, SelectLineEnd,
        SelectDocStart, SelectDocEnd, SelectAll, PageUp, PageDown, Backspace, Delete, DeleteWordLeft, DeleteWordRight,
        Newline, Tab, Undo, Redo, Copy, Cut, Paste, Save
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
        KeyBinding::new("shift-enter", Newline, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("ctrl-s", Save, c),
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
    measured_once: bool,
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
            measured_once: false,
            perf: Perf::default(),
        };
        this.proj.version = u64::MAX;
        this
    }

    // ---- external changes -------------------------------------------------

    /// Something outside this note changed (e.g. a linked file appeared): rebuild the projection.
    pub fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.proj.version = u64::MAX;
        cx.notify();
    }

    /// The workspace's vault watcher saw this file change.
    pub fn on_disk_changed(&mut self, cx: &mut Context<Self>) {
        let Ok(l) = file::load(&self.path) else { return };
        if Some(l.disk) == self.disk {
            return; // our own save, or no real change
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
        if self.status.as_ref().is_some_and(|s| s.starts_with("Cannot open")) {
            return;
        }
        if self.perf.pending_edit.is_none() {
            self.perf.pending_edit = Some(Instant::now());
        }
        self.doc.replace(range, text, kind);
        self.goal_x = None;
        self.autoscroll = true;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_selection(&mut self, sel: Selection, keep_goal: bool, cx: &mut Context<Self>) {
        self.doc.set_selection(sel);
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

    pub(crate) fn projection(&mut self, cx: &App) -> &Projection {
        if self.proj.version != self.doc.version() || self.proj.sel != self.doc.selection() {
            let t = Instant::now();
            let text = self.doc.text();
            let vault = self.vault.borrow();
            let path = &self.path;
            let resolve = |target: &str| vault.resolve(target, path);
            let theme = cx.theme();
            self.proj = projection::project(&ProjectionInput {
                text: &text,
                sel: self.doc.selection(),
                version: self.doc.version(),
                palette: Palette { highlight: theme.yellow.opacity(0.35) },
                resolve: &resolve,
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
            .on_action(cx.listener(|e, _: &MoveUp, _, cx| e.vertical(-1, false, false, cx)))
            .on_action(cx.listener(|e, _: &MoveDown, _, cx| e.vertical(1, false, false, cx)))
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
            .on_action(cx.listener(|e, _: &Backspace, _, cx| e.delete(-1, false, cx)))
            .on_action(cx.listener(|e, _: &Delete, _, cx| e.delete(1, false, cx)))
            .on_action(cx.listener(|e, _: &DeleteWordLeft, _, cx| e.delete(-1, true, cx)))
            .on_action(cx.listener(|e, _: &DeleteWordRight, _, cx| e.delete(1, true, cx)))
            .on_action(cx.listener(|e, _: &Newline, _, cx| e.newline(cx)))
            .on_action(cx.listener(|e, _: &Tab, _, cx| {
                let r = e.doc.selection().range();
                e.edit(r, "\t", EditKind::Insert, cx)
            }))
            .on_action(cx.listener(|e, _: &Undo, _, cx| {
                if e.doc.undo() {
                    e.autoscroll = true;
                    e.schedule_save(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|e, _: &Redo, _, cx| {
                if e.doc.redo() {
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
        let proj = &self.proj;
        // Links open on Ctrl+click only; a plain click places the caret (handled by on_mouse_down).
        // Without a handler gpui-kit would open every link on a plain click.
        let weak = cx.entity().downgrade();
        let mut rcx = RenderContext::from_app(cx)
            .with_markdown_extensions(extensions)
            .with_body_font_size(px(16.))
            .on_link_click(move |url, ev, _, cx| {
                if ev.modifiers().secondary(cx) {
                    let url = url.to_string();
                    let new_tab = ev.modifiers().shift;
                    let _ = weak.update(cx, |e, cx| e.open_link(&url, new_tab, cx));
                }
            });
        for (k, ranges) in &proj.muted {
            rcx = rcx.with_foreground_ranges(*k, ranges.iter().map(|r| (r.clone(), muted)).collect());
        }
        let decorations: HashMap<u64, Vec<(Range<usize>, Hsla)>> =
            proj.selection_ranges(&sel_range).into_iter().map(|(k, v)| (k, v.into_iter().map(|r| (r, sel_color)).collect())).collect();
        let refs: Vec<&gpui_kit::base::text::editor_host::BlockNode> = proj.blocks.iter().map(|b| &b.node).collect();
        let n = refs.len();
        // ---- virtualization: render the blocks within a screen of the viewport (plus the caret's)
        let caret_key = proj.locate(self.doc.selection().head).map(|(k, _)| k);
        let caret_ix = caret_key.and_then(|k| proj.blocks.iter().position(|b| b.keys.contains(&k)));
        let view_h = f32::from(self.scroll.bounds().size.height);
        let top = -f32::from(self.scroll.offset().y);
        let heights = self.heights.borrow();
        const EST: f32 = 30.0;
        let h = |ix: usize| heights.get(&proj.blocks[ix].hash).copied().unwrap_or(EST);
        let all_known = proj.blocks.iter().all(|b| heights.contains_key(&b.hash));
        let mut ys = Vec::with_capacity(n + 1);
        let mut y = 24.0; // content top padding (py_6)
        for ix in 0..n {
            ys.push(y);
            y += h(ix);
        }
        ys.push(y);
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
        self.measured_once |= lo == 0 && hi == n;
        let mut children: Vec<AnyElement> = Vec::with_capacity(hi - lo + 2);
        if lo > 0 {
            children.push(div().h(px(ys[lo] - 24.0)).into_any_element());
        }
        for ix in lo..hi {
            let b = &proj.blocks[ix];
            let el = b.node.render(ix, FlowPosition::at(&refs, ix, ix + 1 == n).first(true).last(true), &rcx, window, cx);
            let el = if b.indent > 0.0 { div().pl(px(b.indent)).child(el).into_any_element() } else { el };
            children.push(measured::Measured::new(b.hash, self.heights.clone(), el).into_any_element());
        }
        if hi < n {
            children.push(div().h(px(ys[n] - ys[hi])).into_any_element());
        }
        let content = div().w_full().max_w(px(760.)).mx_auto().px_8().py_6().flex().flex_col().children(children);
        let editor = cx.entity();
        let status = self.status.clone();
        let root = div().id("editor").key_context(CONTEXT).track_focus(&self.focus).size_full().flex().flex_col().bg(theme.background).text_color(theme.foreground);
        self.perf.render_end = Some(Instant::now());
        Self::register_actions(root, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(input::on_mouse_down))
            .on_mouse_move(cx.listener(input::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(input::on_mouse_up))
            .on_drop(cx.listener(|e, paths: &ExternalPaths, window, cx| e.drop_files(paths, window, cx)))
            .when_some(status, |d, s| d.child(div().px_4().py_1().bg(theme.warning.opacity(0.2)).text_sm().child(s)))
            .child(
                div()
                    .id("editor-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(EditorElement::new(editor, content.into_any_element(), decorations, self.scroll.clone())),
            )
    }
}
