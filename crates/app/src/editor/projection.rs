//! Live-preview projection: document text + selection -> gpui-kit block tree.
//!
//! Each text leaf (a keyed `Paragraph` or `CodeBlock`) carries a [`LeafMap`] that maps leaf
//! byte indices to document offsets and back:
//! - `Text`:   leaf bytes are the source bytes (1:1)
//! - `Hidden`: a source range not shown (zero width in the leaf), e.g. `**`, `## `, `[[x|`
//! - `Atom`:   one U+FFFC in the leaf standing for a whole source range (formula, image)
//!
//! A construct that touches the selection is *revealed*: its markers are shown (muted)
//! instead of hidden, and atoms show their source. That is Obsidian's live-preview rule.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use gpui_kit::base::text::editor_host::{BlockNode, CodeBlock, InlineNode, LinkMark, Paragraph, Span, TextMark};
use gpui_kit::base::text::MarkdownNode;
use gpui_kit::Hsla;
use obsidilight_core::markdown::{parse_blocks, parse_inline, BlockKind, Inline, InlineKind};
use obsidilight_core::Selection;

pub const ATOM: &str = "\u{FFFC}";
pub const MATH_NODE: &str = "ol-math";
pub const IMAGE_NODE: &str = "ol-image";

#[derive(Clone, Debug, PartialEq)]
pub struct MathAtom {
    pub tex: String,
    pub display: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageAtom {
    pub path: Option<PathBuf>,
    pub name: String,
    /// Natural size in pixels, if known.
    pub size: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegKind {
    Text,
    Hidden,
    Atom,
    /// A zero-width space standing in for an empty leaf, so it gets real text metrics
    /// (an empty leaf is taller and draws a taller caret). Maps to no source text.
    Pad,
}

/// Zero-width space used by [`SegKind::Pad`].
pub const PAD: &str = "\u{200B}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seg {
    pub kind: SegKind,
    pub leaf: Range<usize>,
    pub src: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeafMap {
    pub segs: Vec<Seg>,
    /// Document range this leaf covers.
    pub src: Range<usize>,
    pub len: usize,
}

impl LeafMap {
    /// Leaf index -> document offset.
    pub fn to_src(&self, idx: usize) -> usize {
        for s in &self.segs {
            if s.kind == SegKind::Text && s.leaf.start <= idx && idx <= s.leaf.end {
                return s.src.start + (idx - s.leaf.start);
            }
        }
        for s in &self.segs {
            if matches!(s.kind, SegKind::Atom | SegKind::Pad) && s.leaf.start <= idx && idx <= s.leaf.end {
                return if idx <= s.leaf.start { s.src.start } else { s.src.end };
            }
        }
        // only hidden segments around: the position right after the last one before idx
        self.segs.iter().filter(|s| s.leaf.end <= idx).map(|s| s.src.end).max().unwrap_or(self.src.start)
    }

    /// Document offset (inside `self.src`) -> leaf index.
    pub fn to_leaf(&self, off: usize) -> usize {
        for s in &self.segs {
            if s.kind == SegKind::Text && s.src.start <= off && off <= s.src.end {
                return s.leaf.start + (off - s.src.start);
            }
        }
        for s in &self.segs {
            if s.kind != SegKind::Text && s.src.start <= off && off <= s.src.end {
                return if off == s.src.end && s.kind == SegKind::Atom { s.leaf.end } else { s.leaf.start };
            }
        }
        if off >= self.src.end { self.len } else { 0 }
    }

    /// The leaf range covering the document range `r` (clipped to this leaf).
    pub fn leaf_range(&self, r: &Range<usize>) -> Option<Range<usize>> {
        let a = r.start.max(self.src.start);
        let b = r.end.min(self.src.end);
        if a >= b {
            return None;
        }
        let (la, lb) = (self.to_leaf(a), self.to_leaf(b));
        (la < lb).then_some(la..lb)
    }
}

/// Colours baked into marks at build time (everything else is applied at render time).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub highlight: Hsla,
}

pub struct ProjBlock {
    pub node: BlockNode,
    /// Left indent in pixels (list nesting).
    pub indent: f32,
    /// Leaf keys rendered by this block.
    pub keys: Vec<u64>,
    /// Identity for caching the block's measured height: source text, reveal state, part.
    pub hash: u64,
}

#[derive(Default)]
pub struct Projection {
    pub blocks: Vec<ProjBlock>,
    /// Leaves in document order.
    pub leaves: Vec<(u64, LeafMap)>,
    index: HashMap<u64, usize>,
    /// Leaf ranges to paint in the muted colour (revealed markers, frontmatter).
    pub muted: HashMap<u64, Vec<Range<usize>>>,
    pub version: u64,
    pub sel: Selection,
}

impl Projection {
    pub fn leaf(&self, key: u64) -> Option<&LeafMap> {
        self.index.get(&key).map(|&i| &self.leaves[i].1)
    }

    /// Document offset -> (leaf key, leaf index). Offsets outside every leaf snap to the nearest one.
    pub fn locate(&self, off: usize) -> Option<(u64, usize)> {
        let i = self.leaves.partition_point(|(_, m)| m.src.end < off);
        let pick = |i: usize| self.leaves.get(i).map(|(k, m)| (*k, m.to_leaf(off)));
        match self.leaves.get(i) {
            Some((_, m)) if m.src.start <= off => pick(i),
            Some(_) if i > 0 => {
                // between two leaves: the nearer one
                let before = &self.leaves[i - 1].1;
                let after = &self.leaves[i].1;
                if off - before.src.end <= after.src.start - off { pick(i - 1) } else { pick(i) }
            }
            Some(_) => pick(i),
            None => self.leaves.last().map(|(k, m)| (*k, m.len)),
        }
    }

    /// Selection painted per leaf.
    pub fn selection_ranges(&self, r: &Range<usize>) -> HashMap<u64, Vec<Range<usize>>> {
        let mut out = HashMap::new();
        if r.is_empty() {
            return out;
        }
        for (k, m) in &self.leaves {
            if m.src.end < r.start || m.src.start > r.end {
                continue;
            }
            if let Some(lr) = m.leaf_range(r) {
                out.entry(*k).or_insert_with(Vec::new).push(lr);
            }
        }
        out
    }
}

fn key(block: usize, sub: usize) -> u64 {
    ((block as u64) << 8) | sub as u64
}

struct LeafBuilder<'a> {
    text: &'a str,
    sel: Range<usize>,
    pal: Palette,
    para: Paragraph,
    run: String,
    run_marks: Vec<(Range<usize>, TextMark)>,
    map: LeafMap,
    muted: Vec<Range<usize>>,
    /// Source ranges hidden at the text level (quote prefixes), sorted; `.1` = revealed.
    prefixes: Vec<(Range<usize>, bool)>,
    resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
}

impl<'a> LeafBuilder<'a> {
    fn new(text: &'a str, src: Range<usize>, sel: Range<usize>, pal: Palette, resolve: &'a dyn Fn(&str) -> Option<PathBuf>) -> Self {
        Self {
            text,
            sel,
            pal,
            para: Paragraph::default(),
            run: String::new(),
            run_marks: Vec::new(),
            map: LeafMap { segs: Vec::new(), src, len: 0 },
            muted: Vec::new(),
            prefixes: Vec::new(),
            resolve,
        }
    }

    fn revealed(&self, r: &Range<usize>) -> bool {
        self.sel.start <= r.end && self.sel.end >= r.start
    }

    fn flush(&mut self) {
        if !self.run.is_empty() {
            let run = std::mem::take(&mut self.run);
            let marks = std::mem::take(&mut self.run_marks);
            self.para.push(InlineNode::new(run).marks(marks));
        }
    }

    fn push_seg(&mut self, kind: SegKind, leaf: Range<usize>, src: Range<usize>) {
        if let Some(last) = self.map.segs.last_mut() {
            if kind == SegKind::Text && last.kind == SegKind::Text && last.leaf.end == leaf.start && last.src.end == src.start {
                last.leaf.end = leaf.end;
                last.src.end = src.end;
                return;
            }
        }
        self.map.segs.push(Seg { kind, leaf, src });
    }

    /// Visible text, 1:1. Splits out hidden quote prefixes and the `\r` of CRLF.
    fn text(&mut self, src: Range<usize>, mark: &TextMark, muted: bool) {
        let mut cuts: Vec<(Range<usize>, bool)> = self
            .prefixes
            .iter()
            .filter(|(p, _)| p.start < src.end && p.end > src.start)
            .map(|(p, r)| (p.start.max(src.start)..p.end.min(src.end), *r))
            .collect();
        let slice = &self.text[src.clone()];
        for (i, _) in slice.match_indices('\r') {
            let c = src.start + i;
            if self.text.as_bytes().get(c + 1) == Some(&b'\n') {
                cuts.push((c..c + 1, false));
            }
        }
        cuts.sort_by_key(|c| c.0.start);
        let mut pos = src.start;
        for (c, rev) in cuts {
            if c.start < pos {
                continue;
            }
            if c.start > pos {
                self.visible(pos..c.start, mark, muted);
            }
            if rev {
                self.visible(c.clone(), mark, true);
            } else {
                self.hidden(c.clone());
            }
            pos = c.end;
        }
        if pos < src.end {
            self.visible(pos..src.end, mark, muted);
        }
    }

    fn visible(&mut self, src: Range<usize>, mark: &TextMark, muted: bool) {
        let s = &self.text[src.clone()];
        let start = self.run.len();
        self.run.push_str(s);
        self.run_marks.push((start..start + s.len(), mark.clone()));
        let l = self.map.len;
        if muted {
            self.muted.push(l..l + s.len());
        }
        self.push_seg(SegKind::Text, l..l + s.len(), src);
        self.map.len += s.len();
    }

    fn hidden(&mut self, src: Range<usize>) {
        if src.is_empty() {
            return;
        }
        let l = self.map.len;
        self.push_seg(SegKind::Hidden, l..l, src);
    }

    /// Syntax markers: shown muted when revealed, hidden otherwise.
    fn marker(&mut self, src: Range<usize>, mark: &TextMark, reveal: bool) {
        if src.is_empty() {
            return;
        }
        if reveal {
            self.text(src, mark, true);
        } else {
            self.hidden(src);
        }
    }

    fn atom(&mut self, src: Range<usize>, node: MarkdownNode) {
        self.flush();
        self.para.push(InlineNode::custom(node.text(ATOM)));
        let l = self.map.len;
        self.push_seg(SegKind::Atom, l..l + ATOM.len(), src);
        self.map.len += ATOM.len();
    }

    fn math_atom(&mut self, src: Range<usize>, tex: &str, display: bool) {
        // strip quote prefixes inside multi-line math (`> ` continuation lines)
        let tex = if self.prefixes.is_empty() {
            tex.to_string()
        } else {
            tex.split('\n').map(|l| l.trim_start().trim_start_matches(['>', ' '])).collect::<Vec<_>>().join("\n")
        };
        self.atom(src, MarkdownNode::new(MATH_NODE, MathAtom { tex, display }));
    }

    fn inlines(&mut self, nodes: &[Inline], range: Range<usize>, mark: &TextMark) {
        let mut pos = range.start;
        for n in nodes {
            if n.range.start < pos || n.range.end > range.end {
                continue;
            }
            self.text(pos..n.range.start, mark, false);
            self.inline(n, mark);
            pos = n.range.end;
        }
        self.text(pos..range.end, mark, false);
    }

    fn inline(&mut self, n: &Inline, mark: &TextMark) {
        let reveal = self.revealed(&n.range);
        let open = n.range.start..n.inner.start;
        let close = n.inner.end..n.range.end;
        let styled = |f: &dyn Fn(TextMark) -> TextMark| f(mark.clone());
        match &n.kind {
            InlineKind::Strong | InlineKind::Emph | InlineKind::Highlight | InlineKind::Strike | InlineKind::Code => {
                let hl = self.pal.highlight;
                let m = match n.kind {
                    InlineKind::Strong => styled(&|m| m.bold()),
                    InlineKind::Emph => styled(&|m| m.italic()),
                    InlineKind::Highlight => styled(&|m| m.highlight(hl)),
                    InlineKind::Strike => styled(&|m| m.strikethrough()),
                    _ => styled(&|m| m.code()),
                };
                self.marker(open, &m, reveal);
                if n.kind == InlineKind::Code {
                    self.text(n.inner.clone(), &m, false);
                } else {
                    self.inlines(&n.children, n.inner.clone(), &m);
                }
                self.marker(close, &m, reveal);
            }
            InlineKind::Escape => {
                self.marker(open, mark, reveal);
                self.text(n.inner.clone(), mark, false);
            }
            InlineKind::Comment => self.text(n.range.clone(), mark, true),
            InlineKind::Tag => {
                let m = styled(&|m| m.link(LinkMark { url: "tag:".into(), ..Default::default() }));
                self.text(n.range.clone(), &m, false);
            }
            InlineKind::BlockId => self.marker(n.range.clone(), mark, reveal),
            InlineKind::WikiLink { embed: false } | InlineKind::Link => {
                let target = n.link_target(self.text).to_string();
                let m = styled(&|m| m.link(LinkMark { url: target.clone().into(), ..Default::default() }));
                self.marker(open, &m, reveal);
                if n.kind == InlineKind::Link {
                    self.inlines(&n.children, n.inner.clone(), &m);
                } else {
                    self.text(n.inner.clone(), &m, false);
                }
                self.marker(close, &m, reveal);
            }
            InlineKind::Math { display } => {
                if reveal {
                    let m = styled(&|m| m.code());
                    self.text(open, &m, true);
                    self.text(n.inner.clone(), &m, false);
                    self.text(close, &m, true);
                } else {
                    let tex = self.text[n.inner.clone()].to_string();
                    self.math_atom(n.range.clone(), &tex, *display);
                }
            }
            InlineKind::WikiLink { embed: true } | InlineKind::Image => {
                if reveal {
                    self.text(n.range.clone(), mark, true);
                } else {
                    let target = n.link_target(self.text).to_string();
                    let name = target.split('#').next().unwrap_or(&target).to_string();
                    let path = (self.resolve)(&name);
                    let size = path.as_deref().and_then(crate::embeds::image_size);
                    self.atom(n.range.clone(), MarkdownNode::new(IMAGE_NODE, ImageAtom { path, name, size }));
                }
            }
        }
    }

    fn finish(mut self, key: u64, proj: &mut Projection) -> Paragraph {
        if self.map.len == 0 {
            // nothing visible (blank line, empty list item, all markers hidden)
            let at = self.map.segs.last().map(|s| s.src.end).unwrap_or(self.map.src.start);
            self.run.push_str(PAD);
            self.run_marks.push((0..PAD.len(), TextMark::default()));
            self.map.segs.push(Seg { kind: SegKind::Pad, leaf: 0..PAD.len(), src: at..at });
            self.map.len = PAD.len();
        }
        self.flush();
        if !self.muted.is_empty() {
            proj.muted.insert(key, std::mem::take(&mut self.muted));
        }
        proj.index.insert(key, proj.leaves.len());
        proj.leaves.push((key, self.map));
        self.para.with_leaf_key(key)
    }
}

/// Quote markers (`>`, `> >`, with one following space) at the start of each line of `r`.
fn quote_prefixes(text: &str, r: &Range<usize>, sel: &Range<usize>) -> Vec<(Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut ls = r.start;
    for line in text[r.clone()].split('\n') {
        let b = line.as_bytes();
        let mut k = 0;
        while k < b.len() && b[k] == b' ' && k < 3 {
            k += 1;
        }
        let mut end = k;
        while end < b.len() && b[end] == b'>' {
            end += 1;
            if end < b.len() && b[end] == b' ' {
                end += 1;
            }
        }
        if end > k {
            let le = ls + line.len();
            let rev = sel.start <= le && sel.end >= ls;
            out.push((ls..ls + end, rev));
        }
        ls += line.len() + 1;
    }
    out
}

/// The part of the selection inside a block (revealed markers change its height).
fn sel_signature(sel: &Range<usize>, block: &Range<usize>) -> (usize, usize) {
    if sel.start <= block.end && sel.end >= block.start {
        (sel.start.saturating_sub(block.start), sel.end.min(block.end).saturating_sub(block.start))
    } else {
        (usize::MAX, usize::MAX)
    }
}

pub struct ProjectionInput<'a> {
    pub text: &'a str,
    pub sel: Selection,
    pub version: u64,
    pub palette: Palette,
    pub resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
}

pub fn project(inp: &ProjectionInput) -> Projection {
    let text = inp.text;
    let sel = inp.sel.range();
    let mut proj = Projection { version: inp.version, sel: inp.sel, ..Default::default() };
    let blocks = parse_blocks(text);
    for (bi, b) in blocks.iter().enumerate() {
        let rev = sel.start <= b.range.end && sel.end >= b.range.start;
        let new_leaf = |src: Range<usize>| LeafBuilder::new(text, src, sel.clone(), inp.palette, inp.resolve);
        let mut nodes: Vec<(BlockNode, Vec<u64>)> = Vec::new();
        let mut indent = 0.0;
        match &b.kind {
            BlockKind::Blank | BlockKind::Paragraph | BlockKind::Table | BlockKind::Frontmatter => {
                let k = key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                if b.kind == BlockKind::Frontmatter {
                    lb.text(b.range.clone(), &TextMark::default(), true);
                } else {
                    let inl = parse_inline(text, b.range.clone());
                    lb.inlines(&inl, b.range.clone(), &TextMark::default());
                }
                nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
            }
            BlockKind::Quote => {
                let k = key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                lb.prefixes = quote_prefixes(text, &b.range, &sel);
                let inl = parse_inline(text, b.range.clone());
                lb.inlines(&inl, b.range.clone(), &TextMark::default());
                let para = lb.finish(k, &mut proj);
                nodes.push((BlockNode::Blockquote { children: vec![BlockNode::Paragraph(para)], span: None }, vec![k]));
            }
            BlockKind::Heading { level } => {
                let k = key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                lb.marker(b.marker.clone(), &TextMark::default(), rev);
                let inl = parse_inline(text, b.content.clone());
                lb.inlines(&inl, b.content.clone(), &TextMark::default());
                nodes.push((BlockNode::Heading { level: *level, children: lb.finish(k, &mut proj), span: None }, vec![k]));
            }
            BlockKind::ListItem { ordered, task } => {
                let k = key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                // the bullet/number is drawn by the list; the source marker stays hidden
                lb.hidden(b.marker.clone());
                let inl = parse_inline(text, b.content.clone());
                lb.inlines(&inl, b.content.clone(), &TextMark::default());
                let para = lb.finish(k, &mut proj);
                let start = if *ordered {
                    text[b.marker.clone()].trim().trim_end_matches(['.', ')']).parse::<u32>().ok()
                } else {
                    None
                };
                indent = b.indent as f32 * 6.0;
                nodes.push((
                    BlockNode::List {
                        children: vec![BlockNode::ListItem {
                            children: vec![BlockNode::Paragraph(para)],
                            spread: false,
                            checked: *task,
                            task_marker: None,
                            span: None,
                        }],
                        ordered: *ordered,
                        start,
                        span: None,
                    },
                    vec![k],
                ));
            }
            BlockKind::Code { lang, closed } => {
                let open_line = b.range.start..text[b.range.start..].find('\n').map(|i| b.range.start + i).unwrap_or(b.range.end);
                let open_line = open_line.start..open_line.end - usize::from(text[open_line.clone()].ends_with('\r'));
                let close_line = if *closed { text[..b.range.end].rfind('\n').map(|i| i + 1).unwrap_or(b.range.start)..b.range.end } else { b.range.end..b.range.end };
                let mut keys = Vec::new();
                if rev {
                    let k = key(bi, 1);
                    let mut lb = new_leaf(open_line.clone());
                    lb.text(open_line.clone(), &TextMark::default().code(), true);
                    nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
                }
                let k = key(bi, 0);
                let mut code_map = LeafMap { segs: Vec::new(), src: b.content.clone(), len: b.content.len() };
                if !b.content.is_empty() {
                    code_map.segs.push(Seg { kind: SegKind::Text, leaf: 0..b.content.len(), src: b.content.clone() });
                }
                if !rev {
                    // hidden fences: offsets on them snap into the code leaf
                    code_map.src = b.range.clone();
                    code_map.segs.insert(0, Seg { kind: SegKind::Hidden, leaf: 0..0, src: b.range.start..b.content.start });
                    code_map.segs.push(Seg { kind: SegKind::Hidden, leaf: code_map.len..code_map.len, src: b.content.end..b.range.end });
                }
                proj.index.insert(k, proj.leaves.len());
                proj.leaves.push((k, code_map));
                keys.push(k);
                let code = CodeBlock::new(text[b.content.clone()].to_string().into(), (!lang.is_empty()).then(|| lang.clone().into()), None::<Span>).with_leaf_key(k);
                nodes.push((BlockNode::CodeBlock(code), keys));
                if rev && *closed {
                    let k = key(bi, 2);
                    let mut lb = new_leaf(close_line.clone());
                    lb.text(close_line.clone(), &TextMark::default().code(), true);
                    nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
                }
            }
            BlockKind::MathBlock => {
                let tex = text[b.content.clone()].to_string();
                let k = key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                if rev {
                    lb.text(b.range.clone(), &TextMark::default().code(), false);
                    nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
                    // rendered preview under the source (not editable, no key)
                    let mut preview = Paragraph::default();
                    preview.push(InlineNode::custom(MarkdownNode::new(MATH_NODE, MathAtom { tex, display: true }).text(ATOM)));
                    nodes.push((BlockNode::Paragraph(preview), vec![]));
                } else {
                    lb.math_atom(b.range.clone(), &tex, true);
                    nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
                }
            }
            BlockKind::Rule => {
                let k = key(bi, 0);
                if rev {
                    let mut lb = new_leaf(b.range.clone());
                    lb.text(b.range.clone(), &TextMark::default(), true);
                    nodes.push((BlockNode::Paragraph(lb.finish(k, &mut proj)), vec![k]));
                } else {
                    nodes.push((BlockNode::HorizontalRule { span: None }, vec![]));
                }
            }
        }
        for (part, (node, keys)) in nodes.into_iter().enumerate() {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (&text[b.range.clone()], rev, part, &sel_signature(&sel, &b.range)).hash(&mut h);
            proj.blocks.push(ProjBlock { node, indent, keys, hash: h.finish() });
        }
    }
    proj
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::hsla;

    fn proj(text: &str, caret: usize) -> Projection {
        let none = |_: &str| None;
        project(&ProjectionInput { text, sel: Selection::caret(caret), version: 0, palette: Palette { highlight: hsla(0.15, 1., 0.5, 0.4) }, resolve: &none })
    }

    /// The leaf text the renderer sees (atoms as U+FFFC).
    fn leaf_text(p: &Projection, k: u64) -> String {
        let block = p.blocks.iter().find(|b| b.keys.contains(&k)).unwrap();
        fn para_text(n: &BlockNode, out: &mut String) {
            match n {
                BlockNode::Paragraph(p) | BlockNode::Heading { children: p, .. } => {
                    for c in p.children() {
                        out.push_str(if c.as_custom().is_some() { ATOM } else { c.text() });
                    }
                }
                BlockNode::List { children, .. } | BlockNode::ListItem { children, .. } | BlockNode::Blockquote { children, .. } => {
                    children.iter().for_each(|c| para_text(c, out))
                }
                BlockNode::CodeBlock(c) => out.push_str(&c.code()),
                _ => {}
            }
        }
        let mut s = String::new();
        para_text(&block.node, &mut s);
        s
    }

    #[test]
    fn hidden_markers_and_reveal() {
        let t = "a **b** c\n";
        let p = proj(t, t.len()); // caret on the blank last line: nothing revealed in line 1
        let (k, m) = &p.leaves[0];
        assert_eq!(leaf_text(&p, *k), "a b c");
        assert_eq!(m.to_src(2), 2); // boundary before hidden "**": stays outside the bold
        assert_eq!(m.to_leaf(4), 2);
        assert_eq!(m.to_leaf(2), 2); // on the hidden "**" -> its leaf position
        let p = proj(t, 3); // caret inside the opening marker: revealed
        assert_eq!(leaf_text(&p, p.leaves[0].0), "a **b** c");
        assert_eq!(p.muted[&p.leaves[0].0], vec![2..4, 5..7]);
    }

    #[test]
    fn math_is_an_atom_until_revealed() {
        let t = "x $y^2$ z\n\nend";
        let p = proj(t, t.len());
        let (k, m) = &p.leaves[0];
        assert_eq!(leaf_text(&p, *k), "x \u{FFFC} z");
        assert_eq!(m.to_src(2), 2);
        assert_eq!(m.to_src(5), 7);
        assert_eq!(m.to_leaf(4), 2); // inside the formula snaps to its start
        let p = proj(t, 4);
        assert_eq!(leaf_text(&p, p.leaves[0].0), "x $y^2$ z");
    }

    #[test]
    fn heading_marker_and_lists() {
        let t = "## Titolo\n- voce **x**\n";
        let p = proj(t, t.len());
        assert_eq!(leaf_text(&p, p.leaves[0].0), "Titolo");
        assert_eq!(leaf_text(&p, p.leaves[1].0), "voce x");
        assert_eq!(p.leaves[1].1.to_src(0), 12);
        let p = proj(t, 1);
        assert_eq!(leaf_text(&p, p.leaves[0].0), "## Titolo");
    }

    #[test]
    fn display_math_block_and_code_block() {
        let t = "$$\na\\\\b\n$$\n```c\nint x;\n```\n";
        let p = proj(t, t.len());
        assert_eq!(leaf_text(&p, p.leaves[0].0), ATOM);
        assert_eq!(p.leaves[0].1.to_src(0), 0);
        assert_eq!(p.leaves[0].1.to_src(3), 10); // end of the closing $$ line
        assert_eq!(leaf_text(&p, p.leaves[1].0), "int x;");
        // caret on the hidden fence line maps into the code leaf
        assert_eq!(p.locate(12), Some((p.leaves[1].0, 0)));
        let p = proj(t, 2); // inside the math block: source + preview
        assert_eq!(leaf_text(&p, p.leaves[0].0), "$$\na\\\\b\n$$");
        assert_eq!(p.blocks.len(), 4); // source, preview, code block, trailing blank line
    }

    #[test]
    fn quote_prefixes_hidden_and_math_inside() {
        let t = "> [!thm] T\n> $$\n> x\n> $$\n\nend";
        let p = proj(t, t.len());
        let s = leaf_text(&p, p.leaves[0].0);
        assert_eq!(s, "[!thm] T\n\u{FFFC}");
        let p = proj(t, 3);
        assert!(leaf_text(&p, p.leaves[0].0).starts_with("> [!thm] T\n"));
    }

    #[test]
    fn empty_lines_get_a_pad_mapped_to_the_line() {
        let t = "a\n\n- \nb";
        let p = proj(t, 0);
        let blank = &p.leaves[1].1;
        assert_eq!(leaf_text(&p, p.leaves[1].0), PAD);
        assert_eq!(blank.to_src(0), 2);
        assert_eq!(blank.to_src(PAD.len()), 2);
        assert_eq!(blank.to_leaf(2), 0);
        let item = &p.leaves[2].1; // "- " with nothing after the hidden marker
        assert_eq!(item.to_src(0), 5);
        assert_eq!(item.to_leaf(5), 0);
        assert!(p.selection_ranges(&(0..t.len())).get(&p.leaves[1].0).is_none());
    }

    #[test]
    fn crlf_cr_is_hidden() {
        let t = "uno\r\ndue";
        let p = proj(t, 0);
        let (k, m) = &p.leaves[0];
        assert_eq!(leaf_text(&p, *k), "uno\ndue");
        assert_eq!(m.to_src(4), 5);
        assert_eq!(m.to_leaf(5), 4);
    }

    #[test]
    fn selection_ranges_map_per_leaf() {
        let t = "ab **cd**\n\nef";
        let p = proj(t, t.len());
        let r = p.selection_ranges(&(1..12));
        assert_eq!(r[&p.leaves[0].0], vec![1..5]); // "ab cd": the hidden closing ** maps to the leaf end
        assert!(!r.contains_key(&p.leaves[1].0)); // the blank line between has no width to paint
        assert_eq!(r[&p.leaves[2].0], vec![0..1]);
    }

    #[test]
    fn locate_every_offset() {
        let t = "# H\n\n- a $x$\n\n```\nc\n```\nfine";
        let p = proj(t, 0);
        for off in 0..=t.len() {
            if t.is_char_boundary(off) {
                assert!(p.locate(off).is_some(), "{off}");
            }
        }
    }
}
