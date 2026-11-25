//! The document buffer: exact file text in a rope, one selection, undo/redo.
//!
//! All offsets are UTF-8 byte offsets into the document and always lie on char
//! boundaries. The rope stores the text exactly as loaded (CRLF, BOM, trailing
//! whitespace), so a document that is never edited serialises to the same bytes.

use std::ops::Range;
use std::time::{Duration, Instant};

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

/// A selection: `anchor` stays put while `head` (the caret) moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self { anchor: at, head: at }
    }
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }
}

/// What produced an edit; decides how edits group into one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Typing characters: consecutive inserts group.
    Insert,
    /// Backspace/delete: consecutive deletes group.
    Delete,
    /// Paste, snippet expansion, programmatic change: always its own undo step.
    Other,
}

#[derive(Clone, Debug)]
struct Change {
    /// Byte range in the document *before* this change.
    at: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct Transaction {
    changes: Vec<Change>,
    sel_before: Selection,
    sel_after: Selection,
    kind: EditKind,
    last_time: Instant,
}

/// Edits within this window (and contiguous) merge into one undo step.
const GROUP_WINDOW: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug)]
pub struct Document {
    rope: Rope,
    sel: Selection,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Incremented on every content change (including undo/redo).
    version: u64,
    saved_version: u64,
    /// Set when the next edit must not merge into the previous undo step.
    break_group: bool,
}

impl Document {
    pub fn new(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            sel: Selection::caret(0),
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            saved_version: 0,
            break_group: true,
        }
    }

    // ---- reading -------------------------------------------------------

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
    pub fn slice(&self, range: Range<usize>) -> String {
        let r = self.clamp_range(range);
        self.rope.byte_slice(r).to_string()
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }
    pub fn selection(&self) -> Selection {
        self.sel
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ---- selection -----------------------------------------------------

    /// Sets the selection (clamped to char boundaries). Moving the caret ends the current undo group.
    pub fn set_selection(&mut self, sel: Selection) {
        let sel = Selection::new(self.clamp(sel.anchor), self.clamp(sel.head));
        if sel != self.sel {
            self.break_group = true;
        }
        self.sel = sel;
    }

    /// Clamps to the document and moves back to the nearest char boundary.
    pub fn clamp(&self, offset: usize) -> usize {
        let o = offset.min(self.len());
        let c = self.rope.byte_to_char(o);
        self.rope.char_to_byte(c)
    }
    fn clamp_range(&self, r: Range<usize>) -> Range<usize> {
        let s = self.clamp(r.start);
        s..self.clamp(r.end).max(s)
    }

    // ---- editing -------------------------------------------------------

    /// Replaces `range` with `text`, leaves the caret after the inserted text, and records undo.
    pub fn replace(&mut self, range: Range<usize>, text: &str, kind: EditKind) {
        self.replace_at(range, text, kind, Instant::now());
    }

    /// Like [`replace`](Self::replace) with an explicit timestamp (for tests and replay).
    pub fn replace_at(&mut self, range: Range<usize>, text: &str, kind: EditKind, now: Instant) {
        let range = self.clamp_range(range);
        if range.is_empty() && text.is_empty() {
            return;
        }
        let sel_before = self.sel;
        let removed = self.slice(range.clone());
        self.apply(range.start, &removed, text);
        let sel_after = Selection::caret(range.start + text.len());
        self.sel = sel_after;
        let change = Change { at: range.start, removed, inserted: text.to_string() };
        self.redo.clear();
        if let Some(last) = self.undo.last_mut().filter(|_| !self.break_group) {
            if Self::can_merge(last, &change, kind, now) {
                last.changes.push(change);
                last.sel_after = sel_after;
                last.last_time = now;
                return;
            }
        }
        self.undo.push(Transaction { changes: vec![change], sel_before, sel_after, kind, last_time: now });
        self.break_group = kind == EditKind::Other;
    }

    /// Applies a change as part of the previous undo step (mirrored tabstop typing). The
    /// selection is shifted by the change rather than moved to it.
    pub fn replace_joined(&mut self, range: Range<usize>, text: &str) {
        let range = self.clamp_range(range);
        if range.is_empty() && text.is_empty() {
            return;
        }
        let removed = self.slice(range.clone());
        self.apply(range.start, &removed, text);
        let shift = |p: usize| {
            if p >= range.end {
                p + text.len() - removed.len()
            } else if p > range.start {
                range.start + text.len()
            } else {
                p
            }
        };
        self.sel = Selection::new(shift(self.sel.anchor), shift(self.sel.head));
        let change = Change { at: range.start, removed, inserted: text.to_string() };
        self.redo.clear();
        match self.undo.last_mut() {
            Some(last) => {
                last.changes.push(change);
                last.sel_after = self.sel;
            }
            None => {
                let now = Instant::now();
                self.undo.push(Transaction { changes: vec![change], sel_before: self.sel, sel_after: self.sel, kind: EditKind::Other, last_time: now });
            }
        }
    }

    /// Replaces the current selection.
    pub fn replace_selection(&mut self, text: &str, kind: EditKind) {
        let r = self.sel.range();
        self.replace(r, text, kind);
    }

    fn can_merge(last: &Transaction, c: &Change, kind: EditKind, now: Instant) -> bool {
        if kind == EditKind::Other || last.kind != kind || now.duration_since(last.last_time) > GROUP_WINDOW {
            return false;
        }
        let Some(prev) = last.changes.last() else { return false };
        match kind {
            // typing continues right where the previous insert ended; a new word after a space starts a new step
            EditKind::Insert => {
                c.removed.is_empty()
                    && prev.removed.is_empty()
                    && c.at == prev.at + prev.inserted.len()
                    && !(prev.inserted.ends_with(char::is_whitespace) && !c.inserted.starts_with(char::is_whitespace))
            }
            // backspace (removing just before) or delete (removing at the same spot)
            EditKind::Delete => {
                c.inserted.is_empty() && prev.inserted.is_empty() && (c.at + c.removed.len() == prev.at || c.at == prev.at)
            }
            EditKind::Other => false,
        }
    }

    fn apply(&mut self, at: usize, removed: &str, inserted: &str) {
        let c0 = self.rope.byte_to_char(at);
        let c1 = self.rope.byte_to_char(at + removed.len());
        self.rope.remove(c0..c1);
        self.rope.insert(c0, inserted);
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(tx) = self.undo.pop() else { return false };
        for c in tx.changes.iter().rev() {
            self.apply(c.at, &c.inserted, &c.removed);
        }
        self.sel = tx.sel_before;
        self.redo.push(tx);
        self.break_group = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(tx) = self.redo.pop() else { return false };
        for c in &tx.changes {
            self.apply(c.at, &c.removed, &c.inserted);
        }
        self.sel = tx.sel_after;
        self.undo.push(tx);
        self.break_group = true;
        true
    }

    /// Applies a change made elsewhere (the same note edited in another pane). It is not an
    /// undo step here: the selection and this document's own undo history are mapped through
    /// it, so undo keeps undoing only this pane's edits (as CM6 does for remote changes).
    /// History older than an overlapping change is dropped, since it can no longer apply.
    pub fn apply_remote(&mut self, range: Range<usize>, text: &str) {
        let range = self.clamp_range(range);
        if range.is_empty() && text.is_empty() {
            return;
        }
        let removed_len = range.end - range.start;
        let removed = self.slice(range.clone());
        self.apply(range.start, &removed, text);
        let map = |p: usize, r: &Range<usize>, ins: usize| -> usize {
            if p >= r.end {
                p + ins - (r.end - r.start)
            } else if p > r.start {
                r.start + ins
            } else {
                p
            }
        };
        self.sel = Selection::new(map(self.sel.anchor, &range, text.len()), map(self.sel.head, &range, text.len()));
        // walk the history from newest to oldest, carrying the remote change back in time
        let (mut r, ins) = (range, text.len());
        let mut keep = self.undo.len();
        'tx: for (ti, tx) in self.undo.iter_mut().enumerate().rev() {
            tx.sel_after = Selection::new(map(tx.sel_after.anchor, &r, ins), map(tx.sel_after.head, &r, ins));
            for c in tx.changes.iter_mut().rev() {
                let (c_end_after, delta) = (c.at + c.inserted.len(), c.inserted.len() as isize - c.removed.len() as isize);
                if r.end <= c.at {
                    // the remote change lies before this one: shift it
                    c.at = (c.at as isize + ins as isize - removed_len as isize) as usize;
                } else if r.start >= c_end_after {
                    // after it: in the state before this change, the remote range sits earlier
                    r = (r.start as isize - delta) as usize..(r.end as isize - delta) as usize;
                } else {
                    keep = ti + 1; // overlaps: this step and older ones cannot be undone any more
                    break 'tx;
                }
            }
            tx.sel_before = Selection::new(map(tx.sel_before.anchor, &r, ins), map(tx.sel_before.head, &r, ins));
            keep = ti;
        }
        // `keep` = index of the oldest transaction that was mapped successfully
        if keep > 0 {
            self.undo.drain(..keep);
        }
        self.redo.clear();
        self.break_group = true;
    }

    /// The smallest single change turning `old` into `new`: (byte range in `old`, replacement).
    /// Offsets lie on char boundaries. None when the texts are equal.
    pub fn diff(old: &str, new: &str) -> Option<(Range<usize>, String)> {
        if old == new {
            return None;
        }
        let (a, b) = (old.as_bytes(), new.as_bytes());
        let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
            pre -= 1;
        }
        let max_suf = (a.len() - pre).min(b.len() - pre);
        let mut suf = a.iter().rev().zip(b.iter().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
        while !old.is_char_boundary(a.len() - suf) || !new.is_char_boundary(b.len() - suf) {
            suf -= 1;
        }
        Some((pre..a.len() - suf, new[pre..b.len() - suf].to_string()))
    }

    /// Replaces the whole text (external change reload); clears history.
    pub fn reset(&mut self, text: &str) {
        let caret = self.sel.head;
        *self = Self::new(text);
        self.sel = Selection::caret(self.clamp(caret));
    }

    // ---- navigation helpers -------------------------------------------

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }
    /// 0-based line containing `offset`.
    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.byte_to_line(self.clamp(offset))
    }
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.rope.len_lines()))
    }
    /// End of the line's content, before "\n" / "\r\n".
    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 >= self.rope.len_lines() {
            return self.len();
        }
        let next = self.rope.line_to_byte(line + 1);
        let mut end = next - 1; // the '\n'
        if end > 0 && self.rope.byte(end - 1) == b'\r' {
            end -= 1;
        }
        end.max(self.line_start(line))
    }
    pub fn line_text(&self, line: usize) -> String {
        self.slice(self.line_start(line)..self.line_end(line))
    }

    /// Previous grapheme boundary (a line break counts as one step; "\r\n" is one).
    pub fn prev_grapheme(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if offset == 0 {
            return 0;
        }
        let line = self.line_of(offset);
        let ls = self.line_start(line);
        if offset == ls {
            // step over the previous line break
            return self.line_end(line - 1);
        }
        let text = self.slice(ls..offset);
        ls + text.grapheme_indices(true).next_back().map(|(i, _)| i).unwrap_or(0)
    }

    pub fn next_grapheme(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        let line = self.line_of(offset);
        let le = self.line_end(line);
        if offset >= le {
            return if line + 1 < self.line_count() { self.line_start(line + 1) } else { self.len() };
        }
        let text = self.slice(offset..le);
        offset + text.graphemes(true).next().map(str::len).unwrap_or(text.len())
    }

    /// Start of the previous word (skipping whitespace/punctuation first), within the line or to the previous line end.
    pub fn prev_word(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        let line = self.line_of(offset);
        let ls = self.line_start(line);
        if offset == ls {
            return self.prev_grapheme(offset);
        }
        let text = self.slice(ls..offset);
        let mut res = 0;
        for (i, w) in text.split_word_bound_indices() {
            if w.chars().any(char::is_alphanumeric) {
                res = i;
            }
        }
        ls + res
    }

    pub fn next_word(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        let line = self.line_of(offset);
        let le = self.line_end(line);
        if offset >= le {
            return self.next_grapheme(offset);
        }
        let text = self.slice(offset..le);
        for (i, w) in text.split_word_bound_indices() {
            if w.chars().any(char::is_alphanumeric) {
                return offset + i + w.len();
            }
        }
        le
    }

    /// The word around `offset` (for double-click), or an empty range at it.
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let offset = self.clamp(offset);
        let line = self.line_of(offset);
        let ls = self.line_start(line);
        let text = self.line_text(line);
        for (i, w) in text.split_word_bound_indices() {
            let (s, e) = (ls + i, ls + i + w.len());
            if s <= offset && offset < e || (offset == e && e == ls + text.len()) {
                if w.chars().any(char::is_alphanumeric) {
                    return s..e;
                }
            }
        }
        offset..offset
    }

    // ---- UTF-16 (platform text input) ---------------------------------

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let c = self.rope.byte_to_char(self.clamp(offset));
        self.rope.char_to_utf16_cu(c)
    }
    pub fn utf16_to_offset(&self, utf16: usize) -> usize {
        let u = utf16.min(self.rope.len_utf16_cu());
        self.rope.char_to_byte(self.rope.utf16_cu_to_char(u))
    }
    pub fn len_utf16(&self) -> usize {
        self.rope.len_utf16_cu()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn untouched_roundtrip_is_exact() {
        let s = "\u{feff}# Títolo\r\nriga  \r\n\r\nè $x$\n";
        assert_eq!(Document::new(s).text(), s);
    }

    #[test]
    fn typing_groups_and_undo_restores_bytes() {
        let orig = "ab\r\ncd";
        let mut d = Document::new(orig);
        let t = t0();
        d.set_selection(Selection::caret(2));
        for (i, ch) in ["x", "y", "z"].iter().enumerate() {
            let at = 2 + i;
            d.replace_at(at..at, ch, EditKind::Insert, t + Duration::from_millis(100 * i as u64));
        }
        assert_eq!(d.text(), "abxyz\r\ncd");
        assert!(d.undo());
        assert_eq!(d.text(), orig);
        assert_eq!(d.selection(), Selection::caret(2));
        assert!(!d.undo());
        assert!(d.redo());
        assert_eq!(d.text(), "abxyz\r\ncd");
    }

    #[test]
    fn word_boundary_starts_new_undo_step() {
        let mut d = Document::new("");
        let t = t0();
        for (i, ch) in "ab cd".chars().enumerate() {
            d.replace_at(i..i, &ch.to_string(), EditKind::Insert, t);
        }
        d.undo();
        assert_eq!(d.text(), "ab ");
        d.undo();
        assert_eq!(d.text(), "");
    }

    #[test]
    fn pause_or_caret_move_breaks_group() {
        let mut d = Document::new("");
        let t = t0();
        d.replace_at(0..0, "a", EditKind::Insert, t);
        d.replace_at(1..1, "b", EditKind::Insert, t + Duration::from_secs(2));
        d.undo();
        assert_eq!(d.text(), "a");
        d.replace_at(1..1, "c", EditKind::Insert, t);
        d.set_selection(Selection::caret(0));
        d.set_selection(Selection::caret(2));
        d.replace_at(2..2, "d", EditKind::Insert, t);
        d.undo();
        assert_eq!(d.text(), "ac");
    }

    #[test]
    fn joined_change_undoes_with_the_previous_step() {
        let mut d = Document::new("(a) (a)");
        d.set_selection(Selection::caret(2));
        d.replace(2..2, "b", EditKind::Insert);
        d.replace_joined(7..7, "b");
        assert_eq!(d.text(), "(ab) (ab)");
        assert_eq!(d.selection(), Selection::caret(3));
        d.replace_joined(0..0, "x"); // before the caret: caret shifts
        assert_eq!(d.selection(), Selection::caret(4));
        assert!(d.undo());
        assert_eq!(d.text(), "(a) (a)");
    }

    #[test]
    fn backspaces_group() {
        let mut d = Document::new("abcd");
        let t = t0();
        d.replace_at(3..4, "", EditKind::Delete, t);
        d.replace_at(2..3, "", EditKind::Delete, t);
        assert_eq!(d.text(), "ab");
        d.undo();
        assert_eq!(d.text(), "abcd");
    }

    #[test]
    fn graphemes_and_crlf() {
        let d = Document::new("aè\r\nb👍🏽");
        assert_eq!(d.next_grapheme(1), 3);
        assert_eq!(d.next_grapheme(3), 5); // over "\r\n"
        assert_eq!(d.prev_grapheme(5), 3);
        assert_eq!(d.next_grapheme(6), d.len()); // emoji + skin tone is one grapheme
        assert_eq!(d.line_end(0), 3);
        assert_eq!(d.line_text(1), "b👍🏽");
    }

    #[test]
    fn words() {
        let d = Document::new("uno, due tre");
        assert_eq!(d.next_word(0), 3);
        assert_eq!(d.next_word(3), 8);
        assert_eq!(d.prev_word(12), 9);
        assert_eq!(d.prev_word(9), 5);
        assert_eq!(d.word_at(6), 5..8);
    }

    #[test]
    fn utf16() {
        let d = Document::new("è👍x");
        assert_eq!(d.offset_to_utf16(2), 1);
        assert_eq!(d.offset_to_utf16(6), 3);
        assert_eq!(d.utf16_to_offset(3), 6);
        assert_eq!(d.len_utf16(), 4);
    }

    #[test]
    fn clamp_to_char_boundary() {
        let d = Document::new("è");
        assert_eq!(d.clamp(1), 0);
        assert_eq!(d.clamp(99), 2);
    }

    #[test]
    fn remote_changes_map_through_local_history() {
        // pane B typed "B" at the end; pane A then typed at the start
        let mut b = Document::new("hello world");
        b.set_selection(Selection::caret(11));
        b.replace(11..11, "!", EditKind::Other);
        b.apply_remote(0..0, ">> ");
        assert_eq!(b.text(), ">> hello world!");
        assert_eq!(b.selection(), Selection::caret(15), "caret shifted by the remote insert");
        assert!(b.undo(), "own edit still undoable");
        assert_eq!(b.text(), ">> hello world", "undo removes only this pane's edit");
        // a remote change after the local edit leaves it in place
        let mut c = Document::new("abc");
        c.replace(0..0, "X", EditKind::Other);
        c.apply_remote(4..4, "Y");
        assert!(c.undo());
        assert_eq!(c.text(), "abcY");
        // overlapping: the local step can no longer be undone
        let mut d = Document::new("abc");
        d.replace(1..2, "ZZ", EditKind::Other);
        d.apply_remote(0..4, "q");
        assert_eq!(d.text(), "q");
        assert!(!d.undo());
        // several steps: only those older than the overlap are dropped
        let mut e = Document::new("0123456789");
        e.replace(0..0, "a", EditKind::Other); // a0123456789
        e.replace(11..11, "z", EditKind::Other); // a0123456789z
        e.apply_remote(0..2, "A"); // overlaps the first step only -> "A123456789z"
        assert_eq!(e.text(), "A123456789z");
        assert!(e.undo());
        assert_eq!(e.text(), "A123456789");
        assert!(!e.undo());
    }

    #[test]
    fn diff_is_minimal_and_char_safe() {
        assert_eq!(Document::diff("abc", "abc"), None);
        assert_eq!(Document::diff("abc", "abXc"), Some((2..2, "X".into())));
        assert_eq!(Document::diff("aaa", "aa"), Some((2..3, String::new())));
        // è (c3 a8) vs é (c3 a9) share a first byte: the change must cover the whole char
        assert_eq!(Document::diff("è", "é"), Some((0..2, "é".into())));
        let (r, t) = Document::diff("ünïcodé x", "ünïcodé yx").unwrap();
        let mut s = "ünïcodé x".to_string();
        s.replace_range(r, &t);
        assert_eq!(s, "ünïcodé yx");
    }
}
