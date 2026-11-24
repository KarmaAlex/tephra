//! Pointer hit-testing, vertical motion and platform text input (IME) for the editor.

use std::ops::Range;

use gpui_kit::base::text::editor_host::{Caret, LeafGeometry};
use gpui_kit::*;
use obsidilight_core::{EditKind, Selection};

use super::Editor;

/// Which way to look when a point falls between leaves.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bias {
    Nearest,
    Down,
    Up,
}

/// Leaf byte index for a window point: the visual line whose box contains `p.y` (found by
/// binary search over caret positions, which are non-decreasing in y), then the nearest
/// caret on that line by x. Snapping y to the line first keeps a click past the right edge
/// of a wrapped row on that row's end instead of the next row.
fn index_at(g: &LeafGeometry, p: Point<Pixels>) -> usize {
    let b = g.bounds();
    let py = f32::from(p.y).clamp(f32::from(b.top()) + 1.0, f32::from(b.bottom()) - 1.0);
    let len = g.len();
    if len == 0 {
        return 0;
    }
    let caret_y = |i: usize| f32::from(g.position_for_caret(Caret::new(i)).y);
    let mut lo = 0;
    let mut hi = len;
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if caret_y(mid) <= py {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let line_top = caret_y(lo);
    let lh = f32::from(g.line_height());
    let y = if py < line_top + lh { line_top + lh * 0.5 } else { py };
    g.index_for_position(point(p.x, px(y)))
}

impl Editor {
    /// Window point -> document offset, using the last frame's leaf geometry.
    pub fn hit_test(&self, p: Point<Pixels>, bias: Bias) -> Option<usize> {
        let mut best: Option<(f32, f32, u64)> = None; // (vertical distance, horizontal distance, key)
        for (key, g) in &self.geometry {
            let b = g.bounds();
            let dy = if p.y < b.top() {
                f32::from(b.top() - p.y)
            } else if p.y > b.bottom() {
                f32::from(p.y - b.bottom())
            } else {
                0.0
            };
            if dy > 0.0 {
                match bias {
                    Bias::Down if b.top() < p.y => continue,
                    Bias::Up if b.bottom() > p.y => continue,
                    _ => {}
                }
            }
            let dx = if p.x < b.left() {
                f32::from(b.left() - p.x)
            } else if p.x > b.right() {
                f32::from(p.x - b.right())
            } else {
                0.0
            };
            if best.is_none_or(|(bdy, bdx, _)| (dy, dx) < (bdy, bdx)) {
                best = Some((dy, dx, *key));
            }
        }
        let (_, _, key) = best?;
        let g = &self.geometry[&key];
        let idx = index_at(g, p);
        Some(self.proj.leaf(key)?.to_src(idx))
    }

    /// Document offset of the character drawn under `p` (not the nearest caret position:
    /// both edges of a short link's text map outside the link once its markers are hidden).
    pub fn char_under(&self, p: Point<Pixels>) -> Option<usize> {
        let (key, g) = self.geometry.iter().find(|(_, g)| g.bounds().contains(&p))?;
        let idx = index_at(g, p);
        let caret = g.position_for_caret(Caret::new(idx));
        let before = p.x < caret.x;
        let off = self.proj.leaf(*key)?.src_of_char(idx, before)?;
        Some(self.doc.clamp(off))
    }

    /// Caret rectangle for a document offset (window coordinates). At a soft wrap the caret
    /// paints at the end of the upper line (`position_for_index`), so clicking past the end
    /// of a line keeps the caret on that line instead of the next one.
    pub fn caret_bounds(&self, offset: usize) -> Option<Bounds<Pixels>> {
        let (key, idx) = self.proj.locate(offset)?;
        let g = self.geometry.get(&key)?;
        let at = g.position_for_index(idx);
        Some(Bounds::new(at, size(px(2.), g.caret_height())))
    }

    /// Up/down by one visual line (`page` = by a viewport height).
    pub(crate) fn vertical(&mut self, dir: i32, select: bool, page: bool, cx: &mut Context<Self>) {
        let s = self.doc.selection();
        let Some(caret) = self.caret_bounds(s.head) else { return };
        let x = *self.goal_x.get_or_insert(caret.left());
        // half a visual line past the caret: the leaf's line height, not the caret's height
        // (code/math lines are taller than their caret, and the old step stayed on the line)
        let line_h = self
            .proj
            .locate(s.head)
            .and_then(|(k, _)| self.geometry.get(&k))
            .map(|g| f32::from(g.line_height()))
            .unwrap_or(f32::from(caret.size.height));
        let step = if page {
            self.scroll.bounds().size.height.max(caret.size.height) * 0.9
        } else {
            px(line_h.max(f32::from(caret.size.height)) * 0.5)
        };
        let y = if dir > 0 { caret.bottom() + step } else { caret.top() - step };
        let bias = if dir > 0 { Bias::Down } else { Bias::Up };
        let head = match self.hit_test(point(x, y), bias) {
            // no line further in that direction: go to the document edge
            Some(h) if h != s.head => h,
            _ => if dir > 0 { self.doc.len() } else { 0 },
        };
        let sel = if select { Selection::new(s.anchor, head) } else { Selection::caret(head) };
        self.set_selection(sel, true, cx);
    }
}

pub fn on_mouse_down(e: &mut Editor, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Editor>) {
    window.focus(&e.focus, cx);
    if ev.modifiers.secondary(cx) {
        return; // Ctrl+click is for opening links; leave the caret where it is
    }
    let Some(off) = e.hit_test(ev.position, Bias::Nearest) else { return };
    let s = e.doc.selection();
    let sel = match ev.click_count {
        2 => {
            let w = e.doc.word_at(off);
            Selection::new(w.start, w.end)
        }
        3 => {
            let line = e.doc.line_of(off);
            Selection::new(e.doc.line_start(line), e.doc.line_end(line))
        }
        _ if ev.modifiers.shift => Selection::new(s.anchor, off),
        _ => Selection::caret(off),
    };
    e.dragging = true;
    e.set_selection(sel, false, cx);
}
pub fn on_mouse_move(e: &mut Editor, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Editor>) {
    if !e.dragging || ev.pressed_button != Some(MouseButton::Left) {
        e.on_hover(ev.position, ev.modifiers.secondary(cx), cx);
        return;
    }
    if let Some(off) = e.hit_test(ev.position, Bias::Nearest) {
        let s = e.doc.selection();
        if off != s.head {
            e.set_selection(Selection::new(s.anchor, off), false, cx);
        }
    }
}

pub fn on_mouse_up(e: &mut Editor, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Editor>) {
    e.dragging = false;
}

impl Editor {
    fn utf16_range(&self, r: Range<usize>) -> Range<usize> {
        self.doc.offset_to_utf16(r.start)..self.doc.offset_to_utf16(r.end)
    }
    fn byte_range(&self, r: Range<usize>) -> Range<usize> {
        self.doc.utf16_to_offset(r.start)..self.doc.utf16_to_offset(r.end)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, range: Range<usize>, adjusted: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r = self.byte_range(range);
        *adjusted = Some(self.utf16_range(r.clone()));
        Some(self.doc.slice(r))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let s = self.doc.selection();
        Some(UTF16Selection { range: self.utf16_range(s.range()), reversed: s.reversed() })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|r| self.utf16_range(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_item(item, cx);
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let r = range.map(|r| self.byte_range(r)).or_else(|| self.marked.clone()).unwrap_or_else(|| self.doc.selection().range());
        let typed = self.marked.is_none() && r == self.doc.selection().range();
        self.marked = None;
        // plain typing (not IME commits or replacements elsewhere) goes through plugin hooks first
        if typed && text.chars().count() == 1 && self.run_char_hooks(text, cx) {
            return;
        }
        if typed && text.chars().count() == 1 && crate::app_settings::AppSettings::flag(cx, "autoPairBrackets") && self.mode != super::ViewMode::Reading {
            use super::assist::{on_type, Pair};
            let ch = text.chars().next().expect("one char");
            match on_type(ch, self.char_before(r.start), self.char_after(r.end), !r.is_empty()) {
                Pair::Insert { text, caret_back } => {
                    self.edit(r, &text, EditKind::Insert, cx);
                    let h = self.doc.selection().head;
                    self.set_selection(Selection::caret(h - caret_back), false, cx);
                    return;
                }
                Pair::StepOver => {
                    self.set_selection(Selection::caret(r.end + ch.len_utf8()), false, cx);
                    return;
                }
                Pair::Wrap { open, close } => {
                    let inner = self.doc.slice(r.clone());
                    self.edit(r.clone(), &format!("{open}{inner}{close}"), EditKind::Other, cx);
                    let s = r.start + open.len_utf8();
                    self.set_selection(Selection::new(s, s + inner.len()), false, cx);
                    return;
                }
                Pair::None => {}
            }
        }
        self.edit(r, text, EditKind::Insert, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, new_text: &str, new_sel: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let r = range.map(|r| self.byte_range(r)).or_else(|| self.marked.clone()).unwrap_or_else(|| self.doc.selection().range());
        self.edit(r.clone(), new_text, EditKind::Insert, cx);
        self.marked = (!new_text.is_empty()).then(|| r.start..r.start + new_text.len());
        if let Some(ns) = new_sel {
            // new_sel is in UTF-16 units relative to new_text
            let to_byte = |u: usize| new_text.char_indices().scan(0usize, |acc, (i, c)| { let at = *acc; *acc += c.len_utf16(); Some((at, i)) }).find(|(at, _)| *at >= u).map(|(_, i)| i).unwrap_or(new_text.len());
            self.doc.set_selection(Selection::new(r.start + to_byte(ns.start), r.start + to_byte(ns.end)));
        }
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        self.caret_bounds(self.doc.utf16_to_offset(range.start))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        self.hit_test(p, Bias::Nearest).map(|o| self.doc.offset_to_utf16(o))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.doc.len_utf16())
    }
}
