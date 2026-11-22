//! What editor hooks (plugins) can do to an editor: read text and selection, edit, insert
//! snippets with tabstops, and query the math context.
//!
//! Also the tabstop session itself: a core facility (the CodeMirror state-field equivalent) that
//! maps tabstop ranges through edits and mirrors typing into repeated tabstops. Everything here
//! runs against a [`Backend`], so the same code drives the live editor and headless tests.

use std::ops::Range;
use std::path::{Path, PathBuf};

use obsidilight_core::markdown::{find_math, parse_blocks, BlockKind};
use obsidilight_core::{Document, EditKind, Selection};

/// Tabstop groups of an expanded snippet: group `index` is the current one; ranges of one group
/// are edited together (mirrors). Reaching the last group ends the session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabstopSession {
    pub groups: Vec<Vec<Range<usize>>>,
    pub index: usize,
}

impl TabstopSession {
    /// Maps every range through a replacement of `at..at+removed` by `inserted` bytes.
    /// Ranges are inclusive at both ends (typing at a tabstop's edge extends it).
    pub fn map(&mut self, at: usize, removed: usize, inserted: usize) {
        let end = at + removed;
        let map_pos = |p: usize, is_end: bool| -> usize {
            if p < at || (p == at && !is_end) {
                p
            } else if p >= end {
                p + inserted - removed
            } else if is_end {
                at + inserted // inside the replaced span
            } else {
                at
            }
        };
        for g in &mut self.groups {
            for r in g.iter_mut() {
                let s = map_pos(r.start, false);
                let e = map_pos(r.end, true).max(s);
                *r = s..e;
            }
        }
    }

    pub fn current(&self) -> Option<&Vec<Range<usize>>> {
        self.groups.get(self.index)
    }

    /// The selection lies inside some tabstop (any group).
    pub fn contains(&self, sel: &Range<usize>) -> bool {
        self.groups.iter().flatten().any(|r| r.start <= sel.start && sel.end <= r.end)
    }
}

/// A document edit with tabstop bookkeeping: maps the session through the change and, when the
/// change is inside a tabstop of the current group, repeats it in that group's mirrors (as part
/// of the same undo step).
pub fn apply_edit(doc: &mut Document, ts: &mut Option<TabstopSession>, range: Range<usize>, text: &str, kind: EditKind) {
    let start = doc.clamp(range.start);
    let end = doc.clamp(range.end).max(start);
    let removed = end - start;
    let mirror = ts.as_ref().and_then(|s| {
        let g = s.current()?;
        if g.len() < 2 {
            return None;
        }
        let i = g.iter().position(|r| r.start <= start && end <= r.end)?;
        Some((i, start - g[i].start))
    });
    doc.replace(start..end, text, kind);
    if let Some(s) = ts.as_mut() {
        s.map(start, removed, text.len());
    }
    let Some((i, rel)) = mirror else { return };
    let n = ts.as_ref().and_then(|s| s.current().map(Vec::len)).unwrap_or(0);
    for j in 0..n {
        if j == i {
            continue;
        }
        let Some(r) = ts.as_ref().and_then(|s| s.current().and_then(|g| g.get(j).cloned())) else { continue };
        let a = (r.start + rel).min(r.end);
        let b = (a + removed).min(r.end);
        doc.replace_joined(a..b, text);
        if let Some(s) = ts.as_mut() {
            s.map(a, b - a, text.len());
        }
    }
}

/// Where the caret is, mathematically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathCtx {
    /// `$$…$$` (block) rather than `$…$`.
    pub display: bool,
    /// Including delimiters.
    pub outer: Range<usize>,
    /// The TeX between the delimiters.
    pub inner: Range<usize>,
}

/// The primitive operations an editor offers to [`EditorApi`].
pub trait Backend {
    fn doc(&self) -> &Document;
    fn tabstops(&mut self) -> &mut Option<TabstopSession>;
    /// An edit through the editor's normal path (undo, autosave, tabstop mapping/mirroring).
    fn edit(&mut self, r: Range<usize>, text: &str, kind: EditKind);
    /// `nav`: a tabstop move (does not end the session).
    fn select(&mut self, sel: Selection, nav: bool);
    fn path(&self) -> &Path;
}

/// Handle passed to editor hooks.
pub struct EditorApi<'a> {
    b: &'a mut dyn Backend,
}

impl<'a> EditorApi<'a> {
    pub fn new(b: &'a mut dyn Backend) -> Self {
        Self { b }
    }

    pub fn doc(&self) -> &Document {
        self.b.doc()
    }
    pub fn text(&self) -> String {
        self.b.doc().text()
    }
    pub fn slice(&self, r: Range<usize>) -> String {
        self.b.doc().slice(r)
    }
    pub fn len(&self) -> usize {
        self.b.doc().len()
    }
    pub fn selection(&self) -> Selection {
        self.b.doc().selection()
    }
    pub fn path(&self) -> &Path {
        self.b.path()
    }

    pub fn set_caret(&mut self, at: usize) {
        self.b.select(Selection::caret(at), false);
    }
    pub fn set_selection(&mut self, sel: Selection) {
        self.b.select(sel, false);
    }
    pub fn replace(&mut self, r: Range<usize>, text: &str, kind: EditKind) {
        self.b.edit(r, text, kind);
    }

    /// Replaces `r` with `text` and starts (or nests into) a tabstop session. `tabstops` are
    /// (group number, range within `text`); groups are visited in ascending number. The
    /// selection goes to the first group, or after the text when there are none.
    pub fn insert_snippet(&mut self, r: Range<usize>, text: &str, tabstops: &[(usize, Range<usize>)], kind: EditKind) {
        let start = self.b.doc().clamp(r.start);
        self.b.edit(start..r.end, text, kind);
        if tabstops.is_empty() {
            self.b.select(Selection::caret(start + text.len()), true);
            return;
        }
        let mut idx: Vec<usize> = tabstops.iter().map(|(i, _)| *i).collect();
        idx.sort_unstable();
        idx.dedup();
        let groups: Vec<Vec<Range<usize>>> =
            idx.iter().map(|i| tabstops.iter().filter(|(j, _)| j == i).map(|(_, r)| start + r.start..start + r.end).collect()).collect();
        let first = groups[0][0].clone();
        let ts = self.b.tabstops();
        match ts.as_mut() {
            // nested expansion inside a tabstop: the new groups come first, then the rest of
            // the outer snippet's
            Some(s) if s.index < s.groups.len() => {
                let at = s.index;
                s.groups.remove(at);
                for (k, g) in groups.into_iter().enumerate() {
                    s.groups.insert(at + k, g);
                }
                s.index = at;
            }
            _ => *ts = Some(TabstopSession { groups, index: 0 }),
        }
        self.b.select(Selection::new(first.start, first.end), true);
        self.end_session_if_last();
    }

    fn end_session_if_last(&mut self) {
        let ts = self.b.tabstops();
        if ts.as_ref().is_some_and(|s| s.index + 1 >= s.groups.len()) {
            *ts = None;
        }
    }

    pub fn has_tabstops(&mut self) -> bool {
        self.b.tabstops().is_some()
    }

    /// Moves to the next (or previous) tabstop group; false when there is none.
    pub fn next_tabstop(&mut self, backwards: bool) -> bool {
        let Some(s) = self.b.tabstops().clone() else { return false };
        let target = if backwards { s.index.checked_sub(1) } else { Some(s.index + 1) };
        let Some(i) = target.filter(|i| *i < s.groups.len()) else { return false };
        let r = s.groups[i][0].clone();
        if let Some(t) = self.b.tabstops().as_mut() {
            t.index = i;
        }
        self.b.select(Selection::new(r.start, r.end), true);
        self.end_session_if_last();
        true
    }

    /// The math the offset is in (between the delimiters, inclusive), if any.
    pub fn math_at(&self, pos: usize) -> Option<MathCtx> {
        math_at(&self.b.doc().text(), pos)
    }

    /// Inside a fenced code block: its language (possibly empty).
    pub fn code_block_at(&self, pos: usize) -> Option<String> {
        let text = self.b.doc().text();
        parse_blocks(&text).into_iter().find(|b| b.content.start <= pos && pos <= b.content.end).and_then(|b| match b.kind {
            BlockKind::Code { lang, .. } => Some(lang),
            _ => None,
        })
    }
}

/// The math the offset `pos` is in, with the editor's block/inline rules.
pub fn math_at(text: &str, pos: usize) -> Option<MathCtx> {
    let b = parse_blocks(text).into_iter().find(|b| b.range.start <= pos && pos <= b.range.end)?;
    match b.kind {
        BlockKind::MathBlock => (b.content.start <= pos && pos <= b.content.end).then(|| MathCtx { display: true, outer: b.range.clone(), inner: b.content.clone() }),
        BlockKind::Code { .. } | BlockKind::Frontmatter => None,
        _ => find_math(&text[b.range.clone()]).into_iter().find_map(|m| {
            let inner = m.inner();
            let (is, ie) = (b.range.start + inner.start, b.range.start + inner.end);
            (is <= pos && pos <= ie).then(|| MathCtx { display: m.display, outer: b.range.start + m.range.start..b.range.start + m.range.end, inner: is..ie })
        }),
    }
}

/// An editor without a window (tests, and the snippet test harness).
pub struct Headless {
    pub doc: Document,
    pub tabstops: Option<TabstopSession>,
    pub path: PathBuf,
}

impl Headless {
    pub fn new(text: &str) -> Self {
        Self { doc: Document::new(text), tabstops: None, path: PathBuf::from("test.md") }
    }
    /// Text with `|` marking the caret (or `[`…`]` the selection) — handy in tests.
    pub fn marked(&self) -> String {
        let t = self.doc.text();
        let r = self.doc.selection().range();
        if r.is_empty() {
            format!("{}|{}", &t[..r.start], &t[r.start..])
        } else {
            format!("{}[{}]{}", &t[..r.start], &t[r.clone()], &t[r.end..])
        }
    }
}

impl Backend for Headless {
    fn doc(&self) -> &Document {
        &self.doc
    }
    fn tabstops(&mut self) -> &mut Option<TabstopSession> {
        &mut self.tabstops
    }
    fn edit(&mut self, r: Range<usize>, text: &str, kind: EditKind) {
        apply_edit(&mut self.doc, &mut self.tabstops, r, text, kind);
    }
    fn select(&mut self, sel: Selection, nav: bool) {
        self.doc.set_selection(sel);
        if !nav && self.tabstops.as_ref().is_some_and(|s| !s.contains(&sel.range())) {
            self.tabstops = None;
        }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabstop_mapping() {
        let mut s = TabstopSession { groups: vec![vec![5..5], vec![10..12]], index: 0 };
        s.map(5, 0, 3); // typing "abc" at the first (empty) tabstop: it grows, the next shifts
        assert_eq!(s.groups, vec![vec![5..8], vec![13..15]]);
        s.map(6, 1, 0); // delete inside the first
        assert_eq!(s.groups, vec![vec![5..7], vec![12..14]]);
        s.map(0, 2, 0); // delete before everything
        assert_eq!(s.groups, vec![vec![3..5], vec![10..12]]);
        s.map(11, 3, 1); // replace across the end of the second
        assert_eq!(s.groups[1], vec![10..12]);
    }

    #[test]
    fn snippet_tabstops_navigate_and_mirror() {
        let mut h = Headless::new("x ");
        h.doc.set_selection(Selection::caret(2));
        {
            let mut api = EditorApi::new(&mut h);
            // "\frac{$0}{$1}$2" with mirrored $0 at the end: "\frac{}{}" + "" ...
            let text = "\\frac{}{}=";
            api.insert_snippet(2..2, text, &[(0, 6..6), (1, 8..8), (2, 10..10)], EditKind::Other);
        }
        assert_eq!(h.marked(), "x \\frac{|}{}=");
        h.edit(8..8, "a", EditKind::Insert);
        assert!(EditorApi::new(&mut h).next_tabstop(false));
        assert_eq!(h.marked(), "x \\frac{a}{|}=");
        assert!(h.tabstops.is_some());
        assert!(EditorApi::new(&mut h).next_tabstop(false));
        assert_eq!(h.marked(), "x \\frac{a}{}=|");
        assert!(h.tabstops.is_none(), "reaching the last group ends the session");

        let mut h = Headless::new("");
        EditorApi::new(&mut h).insert_snippet(0..0, "\\begin{}\\end{}$", &[(0, 7..7), (0, 13..13), (1, 14..14)], EditKind::Other);
        let sel = h.doc.selection().head;
        h.edit(sel..sel, "p", EditKind::Insert);
        h.edit(sel + 1..sel + 1, "m", EditKind::Insert);
        assert_eq!(h.doc.text(), "\\begin{pm}\\end{pm}$");
        assert!(h.doc.undo() && h.doc.undo());
        assert_eq!(h.doc.text(), "\\begin{}\\end{}$");
    }

    #[test]
    fn moving_outside_ends_the_session() {
        let mut h = Headless::new("abc ");
        EditorApi::new(&mut h).insert_snippet(4..4, "()()", &[(0, 1..1), (1, 3..3), (2, 4..4)], EditKind::Other);
        assert!(h.tabstops.is_some());
        EditorApi::new(&mut h).set_caret(0);
        assert!(h.tabstops.is_none());
    }

    #[test]
    fn math_context() {
        let t = "a $x+y$ b\n\n$$\n\\frac{1}{2}\n$$\n> $$c$$";
        assert_eq!(math_at(t, 4).map(|m| (m.display, m.inner)), Some((false, 3..6)));
        assert_eq!(math_at(t, 3).map(|m| m.inner), Some(3..6)); // right after the opening $
        assert!(math_at(t, 1).is_none());
        let in_block = t.find("frac").unwrap();
        assert_eq!(math_at(t, in_block).map(|m| m.display), Some(true));
        let in_quote = t.rfind('c').unwrap();
        assert_eq!(math_at(t, in_quote).map(|m| m.display), Some(true));
    }
}
