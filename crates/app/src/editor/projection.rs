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

use gpui_kit::base::text::editor_host::{BlockNode, CodeBlock, ColumnAlign, InlineNode, LinkMark, Paragraph, Span, Table, TextMark};
use gpui_kit::base::text::MarkdownNode;
use gpui_kit::Hsla;
use tephra_core::markdown::callouts::{parse_quote, Callout, QuoteItem};
use tephra_core::markdown::{parse_blocks, parse_inline, parse_table, Align, BlockKind, Inline, InlineKind};
use tephra_core::markdown::{blocks::continues_table, tables::is_table_start};
use tephra_core::Selection;

pub const ATOM: &str = "\u{FFFC}";
pub const MATH_NODE: &str = "ol-math";
pub const IMAGE_NODE: &str = "ol-image";
pub const SUPSUB_NODE: &str = "ol-supsub";

/// `<sup>…</sup>` / `<sub>…</sub>` while not revealed: small text, raised or lowered.
#[derive(Clone, Debug, PartialEq)]
pub struct SupSubAtom {
    pub text: String,
    pub sup: bool,
}

/// Block alignment from the editing toolbar's HTML (`<center>`, `<p align="right">`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockAlign {
    Center,
    Right,
    Justify,
}

/// Text of an inline span with HTML tags and emphasis markers removed (for atoms).
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            '*' | '=' | '~' => {}
            _ => out.push(c),
        }
    }
    out
}

/// How an HTML element formats its content: (mark, text colour).
fn html_format(open_tag: &str, mark: &TextMark, highlight: Hsla) -> (TextMark, Option<Hsla>) {
    use tephra_core::markdown::inline::{html_attr, html_tag_name};
    let m = mark.clone();
    let style = html_attr(open_tag, "style").unwrap_or_default();
    let mut fg = crate::css::style_prop(&style, "color").and_then(|c| crate::css::parse_color(&c));
    let bg = crate::css::style_prop(&style, "background").or_else(|| crate::css::style_prop(&style, "background-color")).and_then(|c| crate::css::parse_color(&c));
    let mut m = match html_tag_name(open_tag).as_str() {
        "u" | "ins" => m.underline(),
        "b" | "strong" => m.bold(),
        "i" | "em" => m.italic(),
        "s" | "del" | "strike" => m.strikethrough(),
        "mark" => m.highlight(highlight),
        "font" => {
            fg = fg.or_else(|| html_attr(open_tag, "color").and_then(|c| crate::css::parse_color(&c)));
            m
        }
        _ => m,
    };
    if let Some(bg) = bg {
        m = m.highlight(bg);
    }
    (m, fg)
}

/// The alignment of a paragraph that is entirely one aligned HTML element.
fn block_align(text: &str, range: &Range<usize>, inl: &[Inline]) -> Option<BlockAlign> {
    use tephra_core::markdown::inline::{html_attr, html_tag_name};
    let t = text[range.clone()].trim_end();
    let [n] = inl else { return None };
    if n.kind != InlineKind::Html || n.range.start != range.start || n.range.end != range.start + t.len() {
        return None;
    }
    let open = &text[n.range.start..n.inner.start];
    let align = match html_tag_name(open).as_str() {
        "center" => "center".to_string(),
        _ => html_attr(open, "align").or_else(|| html_attr(open, "style").and_then(|s| crate::css::style_prop(&s, "text-align")))?.to_ascii_lowercase(),
    };
    match align.as_str() {
        "center" => Some(BlockAlign::Center),
        "right" => Some(BlockAlign::Right),
        "justify" => Some(BlockAlign::Justify),
        _ => None,
    }
}

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
    /// Source offset of the leaf character starting at `idx` (`before`: the one ending there),
    /// for "which character is under the mouse". None at a leaf edge or on a pad.
    pub fn src_of_char(&self, idx: usize, before: bool) -> Option<usize> {
        let seg = self.segs.iter().find(|s| s.kind != SegKind::Hidden && if before { s.leaf.start < idx && idx <= s.leaf.end } else { s.leaf.start <= idx && idx < s.leaf.end })?;
        match seg.kind {
            SegKind::Text => Some(seg.src.start + (if before { idx - 1 } else { idx }) - seg.leaf.start),
            SegKind::Atom => Some(seg.src.start),
            _ => None,
        }
    }

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

/// What a block renders as: a gpui-kit block, or a callout container (drawn by the editor).
pub enum ProjNode {
    Block(BlockNode),
    Callout(CalloutView),
    /// A fenced code block drawn by a plugin's code-block renderer (caret outside it).
    Rendered { lang: String, code: String, at: usize },
    /// A block with a note above it (e.g. "Desmos plugin disabled").
    Noted { note: String, child: BlockNode },
    /// A rendered display formula on its own (may be centred, per the editor setting); `line`
    /// = its first line in the note (where a plugin's equation number is keyed).
    Display { node: BlockNode, line: usize },
    /// A task list item (`- [ ] …`) drawn with a clickable checkbox; `at` = the source range of
    /// `[ ]` / `[x]`. While the caret is on the item it is a plain paragraph showing its source.
    Task { checked: bool, at: Range<usize>, child: BlockNode },
    /// Nodes one under the other with no gap, each indented by its pixels: a callout's run of
    /// text lines and list items (one child of the callout).
    Stack(Vec<(f32, ProjNode)>),
}

/// How the host handles a fenced code block's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeMode {
    Plain,
    /// A loaded plugin renders it.
    Rendered,
    /// Shown as code with this note (its plugin is off or not built yet).
    Note(String),
}

pub struct CalloutView {
    /// Lowercased type (`thm`, `pf`…).
    pub kind: String,
    /// Text after `|` in the brackets.
    pub meta: String,
    pub fold: Option<char>,
    /// Identity for the fold state (header line text + occurrence).
    pub fold_key: String,
    /// The header line as a keyed leaf: marker hidden, title text editable in place.
    pub title: Paragraph,
    pub title_empty: bool,
    pub children: Vec<ProjNode>,
    /// Line of the header (plugins number callouts by line).
    pub line: usize,
    /// Body lines without their quote markers (plugins may render the body themselves).
    pub body: Vec<String>,
    /// Where the body starts (a click on a plugin-drawn body puts the caret there).
    pub body_start: usize,
}

pub struct ProjBlock {
    pub node: ProjNode,
    /// Left indent in pixels (list nesting).
    pub indent: f32,
    /// Leaf keys rendered by this block.
    pub keys: Vec<u64>,
    /// Identity for caching the block's measured height: source text, reveal state, part.
    pub hash: u64,
    /// Stable identity across edits inside the block (`key(block, part)`): when `hash` is new
    /// (typed text), the last height measured for this `id` is a much better estimate than a
    /// constant, so the virtualized layout doesn't jump a frame while the block re-measures.
    pub id: u64,
    /// Text alignment (a paragraph wrapped in `<center>` / `<p align=…>`).
    pub align: Option<BlockAlign>,
}

#[derive(Default)]
pub struct Projection {
    pub blocks: Vec<ProjBlock>,
    /// Leaves in document order.
    pub leaves: Vec<(u64, LeafMap)>,
    index: HashMap<u64, usize>,
    /// Leaf ranges to paint in the muted colour (revealed markers, frontmatter).
    pub muted: HashMap<u64, Vec<Range<usize>>>,
    /// Leaf ranges in a colour of their own (`<font color>`, `<span style="color:…">`).
    pub colors: HashMap<u64, Vec<(Range<usize>, Hsla)>>,
    pub version: u64,
    pub sel: Selection,
    pub mode: ViewMode,
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
    ((block as u64) << 16) | sub as u64
}

/// The leaf-key base of every nested Markdown render (a plugin block's body, a page preview).
/// The editor's own keys (`block << 16 | sub`, base 0) stay far below it, so a nested render
/// never overwrites the editor's leaf geometry; the editor drops keys at or above it from its
/// geometry, since nested text is read-only and never hit-tested.
pub const NESTED_KEY_BASE: u64 = 1 << 40;

/// Display name for a callout type (Obsidian built-ins and the vault's Academic Notes types).
/// `thm` -> `Thm` (Obsidian's title for a callout without one).
pub fn capitalize(kind: &str) -> String {
    let mut c = kind.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Normalises a code fence language for the highlighter (Bison/Yacc/Flex use C: their actions are C).
pub(crate) fn code_lang(lang: &str) -> Option<String> {
    let l = lang.to_lowercase();
    let mapped = match l.as_str() {
        "" => return None,
        "c++" | "cxx" | "cc" | "hpp" => "cpp",
        "bison" | "yacc" | "flex" | "lex" | "h" => "c",
        "sh" | "shell" | "zsh" => "bash",
        "py" => "python",
        "yml" => "yaml",
        "json" | "js" => "javascript",
        other => other,
    };
    Some(mapped.to_string())
}

fn column_align(a: Align) -> ColumnAlign {
    match a {
        Align::None => ColumnAlign::None,
        Align::Left => ColumnAlign::Left,
        Align::Center => ColumnAlign::Center,
        Align::Right => ColumnAlign::Right,
    }
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
    /// Text colour in effect (inside `<font color>`), and the coloured leaf ranges.
    fg: Option<Hsla>,
    colors: Vec<(Range<usize>, Hsla)>,
    /// Source ranges hidden at the text level (quote prefixes), sorted; `.1` = revealed.
    prefixes: Vec<(Range<usize>, bool)>,
    /// Inside a table cell: `\|` in math is a literal pipe (Obsidian's table escaping).
    table_cell: bool,
    /// Revealed math source: empty lines get a zero-width pad in the source style, so an empty
    /// line is as tall before typing as after (otherwise it takes the body font's height).
    pad_empty_lines: bool,
    resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Display text for an unaliased internal link (plugins: "lem 1.1" for a block reference).
    link_label: &'a dyn Fn(&str) -> Option<String>,
}

impl<'a> LeafBuilder<'a> {
    fn new(text: &'a str, src: Range<usize>, sel: Range<usize>, pal: Palette, resolve: &'a dyn Fn(&str) -> Option<PathBuf>, link_label: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            text,
            sel,
            pal,
            para: Paragraph::default(),
            run: String::new(),
            run_marks: Vec::new(),
            map: LeafMap { segs: Vec::new(), src, len: 0 },
            muted: Vec::new(),
            fg: None,
            colors: Vec::new(),
            prefixes: Vec::new(),
            table_cell: false,
            pad_empty_lines: false,
            resolve,
            link_label,
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

    /// Shows `shown` in place of the source `src`, as one unit (the caret steps over it).
    fn replaced(&mut self, src: Range<usize>, shown: &str, mark: &TextMark) {
        let start = self.run.len();
        self.run.push_str(shown);
        self.run_marks.push((start..start + shown.len(), mark.clone()));
        let l = self.map.len;
        self.map.segs.push(Seg { kind: SegKind::Atom, leaf: l..l + shown.len(), src });
        self.map.len += shown.len();
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
                self.pad_if_empty_line(c.end, mark);
            }
            pos = c.end;
        }
        if pos < src.end {
            self.visible(pos..src.end, mark, muted);
        }
    }

    fn visible(&mut self, src: Range<usize>, mark: &TextMark, muted: bool) {
        if self.pad_empty_lines {
            if let Some(nl) = self.text[src.clone()].find('\n').filter(|nl| src.start + nl + 1 < src.end || src.end == src.start + nl + 1) {
                let cut = src.start + nl + 1;
                self.visible_run(src.start..cut, mark, muted);
                self.pad_if_empty_line(cut, mark);
                if cut < src.end {
                    self.visible(cut..src.end, mark, muted);
                }
                return;
            }
        }
        self.visible_run(src, mark, muted);
    }

    /// A pad when the line starting at `pos` has no visible text (see `pad_empty_lines`).
    fn pad_if_empty_line(&mut self, pos: usize, mark: &TextMark) {
        if !self.pad_empty_lines {
            return;
        }
        let at_line_start = pos == 0 || self.text.as_bytes()[pos - 1] == b'\n' || self.prefixes.iter().any(|(p, r)| !r && p.end == pos);
        let rest = &self.text.as_bytes()[pos.min(self.text.len())..];
        let empty = pos >= self.map.src.end || matches!(rest.first(), Some(b'\n' | b'\r'));
        let prefix_follows = self.prefixes.iter().any(|(p, _)| p.start == pos && p.end > pos);
        if at_line_start && empty && !prefix_follows {
            let start = self.run.len();
            self.run.push_str(PAD);
            self.run_marks.push((start..start + PAD.len(), mark.clone()));
            let l = self.map.len;
            self.map.segs.push(Seg { kind: SegKind::Pad, leaf: l..l + PAD.len(), src: pos..pos });
            self.map.len += PAD.len();
        }
    }

    fn visible_run(&mut self, src: Range<usize>, mark: &TextMark, muted: bool) {
        let s = &self.text[src.clone()];
        let start = self.run.len();
        self.run.push_str(s);
        self.run_marks.push((start..start + s.len(), mark.clone()));
        let l = self.map.len;
        if muted {
            self.muted.push(l..l + s.len());
        } else if let Some(c) = self.fg {
            self.colors.push((l..l + s.len(), c));
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
        if let Some(c) = self.fg {
            self.colors.push((l..l + ATOM.len(), c));
        }
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
        let tex = if self.table_cell { tex.replace("\\|", "|") } else { tex };
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
            InlineKind::Html => {
                let open_tag = self.text[open.clone()].to_string();
                let name = tephra_core::markdown::inline::html_tag_name(&open_tag);
                if !reveal && (name == "sup" || name == "sub") {
                    let t = plain(&self.text[n.inner.clone()]);
                    self.atom(n.range.clone(), MarkdownNode::new(SUPSUB_NODE, SupSubAtom { text: t, sup: name == "sup" }));
                    return;
                }
                let (m, fg) = html_format(&open_tag, mark, self.pal.highlight);
                self.marker(open, mark, reveal);
                let was = self.fg;
                if fg.is_some() {
                    self.fg = fg;
                }
                self.inlines(&n.children, n.inner.clone(), &m);
                self.fg = was;
                self.marker(close, mark, reveal);
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
                // wikilinks to notes that don't exist yet are dimmed (Obsidian); Ctrl+click creates them
                let unresolved = n.kind != InlineKind::Link && (self.resolve)(&target).is_none();
                self.marker(open, &m, reveal);
                let label = (n.kind != InlineKind::Link && !reveal && !self.text[n.range.clone()].contains('|')).then(|| (self.link_label)(&target)).flatten();
                if n.kind == InlineKind::Link {
                    self.inlines(&n.children, n.inner.clone(), &m);
                } else if let Some(label) = label {
                    self.replaced(n.inner.clone(), &label, &m);
                } else {
                    self.text(n.inner.clone(), &m, unresolved);
                }
                self.marker(close, &m, reveal);
            }
            InlineKind::Math { display } => {
                if reveal {
                    let m = styled(&|m| m.code());
                    let was = std::mem::replace(&mut self.pad_empty_lines, *display);
                    self.text(open, &m, true);
                    self.text(n.inner.clone(), &m, false);
                    self.text(close, &m, true);
                    self.pad_empty_lines = was;
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
        if !self.colors.is_empty() {
            proj.colors.insert(key, std::mem::take(&mut self.colors));
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

/// Builds rendered callouts (caret outside the quote block).
struct QuoteCtx<'a> {
    text: &'a str,
    sel: Range<usize>,
    palette: Palette,
    resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    link_label: &'a dyn Fn(&str) -> Option<String>,
    bi: usize,
    sub: usize,
    key_base: u64,
    keys: Vec<u64>,
    occurrences: HashMap<String, usize>,
    /// Offsets of the note's newlines (line numbers of formulas).
    newlines: &'a [usize],
}

/// Line number of a byte offset, given the offsets of every `\n`.
fn line_at(newlines: &[usize], off: usize) -> usize {
    newlines.partition_point(|&p| p < off)
}

impl QuoteCtx<'_> {
    fn next_key(&mut self) -> u64 {
        let k = self.key_base + key(self.bi, self.sub);
        self.sub += 1;
        self.keys.push(k);
        k
    }

    fn leaf(&self, src: Range<usize>) -> LeafBuilder<'_> {
        LeafBuilder::new(self.text, src, self.sel.clone(), self.palette, self.resolve, self.link_label)
    }

    /// Callout body lines: GFM tables (unless the caret is in them) become tables; the rest
    /// goes through [`Self::body_text`].
    fn body_lines(&mut self, r: &Range<usize>, proj: &mut Projection) -> Vec<ProjNode> {
        let text = self.text;
        // each line's range without its quote prefix
        let mut lines: Vec<Range<usize>> = Vec::new();
        // lazy continuation lines (no `>`) cannot start or continue a table
        let mut quoted: Vec<bool> = Vec::new();
        let mut ls = r.start;
        for raw in text[r.clone()].split('\n') {
            let full = ls..ls + raw.strip_suffix('\r').unwrap_or(raw).len();
            let pre = text[full.clone()].len() - text[full.clone()].trim_start_matches(|c: char| c == '>' || c == ' ' || c == '\t').len();
            lines.push(full.start + pre..full.end);
            quoted.push(text[full.clone()].trim_start().starts_with('>'));
            ls += raw.len() + 1;
        }
        let line_start = |i: usize| text[..lines[i].start].rfind('\n').map_or(0, |p| p + 1).max(r.start);
        let mut out = Vec::new();
        let mut cur = r.start;
        let mut i = 0;
        while i + 1 < lines.len() {
            // GFM: header + matching delimiter row, then rows up to a blank line or another block
            if !(quoted[i] && quoted[i + 1] && is_table_start(&text[lines[i].clone()], &text[lines[i + 1].clone()])) {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            while j < lines.len() && quoted[j] && continues_table(&text[lines[j].clone()]) {
                j += 1;
            }
            let (ts, te) = (line_start(i), lines[j - 1].end);
            let revealed = self.sel.start <= te && self.sel.end >= ts;
            let Some(t) = (!revealed).then(|| tephra_core::markdown::tables::parse_table_lines(text, &lines[i..j])).flatten() else {
                i = j;
                continue;
            };
            if ts > cur {
                out.extend(self.body_text(&(cur..ts.saturating_sub(1).max(cur)), proj));
            }
            let cols = t.rows[0].len();
            let rows: Vec<Vec<Paragraph>> = t
                .rows
                .iter()
                .map(|row| {
                    (0..cols)
                        .map(|c| match row.get(c) {
                            Some(cell) => {
                                let k = self.next_key();
                                let mut lb = self.leaf(cell.clone());
                                lb.table_cell = true;
                                let inl = parse_inline(text, cell.clone());
                                lb.inlines(&inl, cell.clone(), &TextMark::default());
                                lb.finish(k, proj)
                            }
                            None => Paragraph::default(),
                        })
                        .collect()
                })
                .collect();
            let aligns = t.aligns.iter().map(|a| column_align(*a)).collect();
            out.push(ProjNode::Block(BlockNode::Table(Table::new(rows, aligns))));
            cur = (te + 1).min(r.end);
            i = j;
        }
        if cur < r.end || out.is_empty() {
            out.extend(self.body_text(&(cur..r.end), proj));
        }
        out
    }

    /// Callout body text, with each unrevealed display formula that sits on its own lines
    /// split out as a `Display` block (so it can be centred like top-level display math).
    fn body_text(&mut self, r: &Range<usize>, proj: &mut Projection) -> Vec<ProjNode> {
        let text = self.text;
        let mut out = Vec::new();
        let mut cur = r.start;
        for m in tephra_core::markdown::find_math(&text[r.clone()]).into_iter().filter(|m| m.display) {
            let (ms, me) = (r.start + m.range.start, r.start + m.range.end);
            let ls = text[..ms].rfind('\n').map_or(0, |i| i + 1).max(r.start);
            let le = text[me..].find('\n').map_or(r.end, |i| me + i).min(r.end);
            let only_prefix_before = text[ls..ms].chars().all(|c| c == '>' || c == ' ' || c == '\t');
            let only_space_after = text[me..le].trim().is_empty();
            let revealed = self.sel.start <= me && self.sel.end >= ms;
            if ls < cur || !only_prefix_before || !only_space_after || revealed {
                continue;
            }
            if ls > cur + 1 {
                out.push(self.text_run(&(cur..ls - 1), proj));
            }
            match self.lines(&(ls..le), proj) {
                ProjNode::Block(b) => out.push(ProjNode::Display { node: b, line: line_at(self.newlines, ls) }),
                other => out.push(other),
            }
            cur = (le + 1).min(r.end);
        }
        if cur < r.end || out.is_empty() {
            out.push(self.text_run(&(cur..r.end), proj));
        }
        out
    }

    /// A run of callout body lines, as Obsidian renders them: list items (tasks included) drawn
    /// as the top level draws them, indented by nesting; the other lines as paragraphs. One
    /// node, so the callout's gap between children doesn't come between them.
    fn text_run(&mut self, r: &Range<usize>, proj: &mut Projection) -> ProjNode {
        let text = self.text;
        let mut items: Vec<(f32, ProjNode)> = Vec::new();
        let mut para: Option<Range<usize>> = None; // lines not in a list, gathered
        let mut ls = r.start;
        for raw in text[r.clone()].split('\n') {
            let le = ls + raw.strip_suffix('\r').unwrap_or(raw).len();
            let line = ls..le;
            ls += raw.len() + 1;
            // the content after the quote markers, parsed as a line of its own
            let cs = tephra_core::markdown::callouts::quote_prefixes(text, &line).last().map_or(line.start, |p| p.end);
            let item = parse_blocks(&text[cs..le]).into_iter().next().filter(|b| matches!(b.kind, BlockKind::ListItem { .. }));
            let Some(b) = item else {
                para = Some(para.map_or(line.clone(), |p| p.start..line.end));
                continue;
            };
            if let Some(p) = para.take() {
                items.push((0.0, self.lines(&p, proj)));
            }
            let (marker, content) = (cs + b.marker.start..cs + b.marker.end, cs + b.content.start..cs + b.content.end);
            let k = self.next_key();
            let mut lb = self.leaf(line.clone());
            lb.prefixes = tephra_core::markdown::callouts::quote_prefixes(text, &line).into_iter().map(|p| (p, false)).collect();
            let inl = parse_inline(text, content.clone());
            let indent = b.indent as f32 * 6.0;
            let node = match b.kind {
                BlockKind::ListItem { task: Some(done), .. } => {
                    lb.marker(marker.clone(), &TextMark::default(), false);
                    let start = lb.map.len;
                    lb.inlines(&inl, content.clone(), &if done { TextMark::default().strikethrough() } else { TextMark::default() });
                    if done {
                        let end = lb.map.len;
                        lb.muted.push(start..end);
                    }
                    let box_at = text[marker.clone()].find('[').map_or(marker.start, |i| marker.start + i);
                    ProjNode::Task { checked: done, at: box_at..box_at + 3, child: BlockNode::Paragraph(lb.finish(k, proj)) }
                }
                BlockKind::ListItem { ordered, task } => {
                    lb.hidden(marker.clone());
                    lb.inlines(&inl, content.clone(), &TextMark::default());
                    let start = if ordered { text[marker.clone()].trim().trim_end_matches(['.', ')']).parse::<u32>().ok() } else { None };
                    let para = lb.finish(k, proj);
                    ProjNode::Block(BlockNode::List {
                        children: vec![BlockNode::ListItem { children: vec![BlockNode::Paragraph(para)], spread: false, checked: task, task_marker: None, span: None }],
                        ordered,
                        start,
                        span: None,
                    })
                }
                _ => unreachable!("filtered to list items"),
            };
            items.push((indent, node));
        }
        if let Some(p) = para.take() {
            items.push((0.0, self.lines(&p, proj)));
        }
        match items.len() {
            1 if items[0].0 == 0.0 => items.pop().map(|(_, n)| n).expect("one"),
            _ => ProjNode::Stack(items),
        }
    }

    fn lines(&mut self, r: &Range<usize>, proj: &mut Projection) -> ProjNode {
        let k = self.next_key();
        let mut lb = self.leaf(r.clone());
        lb.prefixes = tephra_core::markdown::callouts::quote_prefixes(self.text, r).into_iter().map(|p| (p, false)).collect();
        let inl = parse_inline(self.text, r.clone());
        lb.inlines(&inl, r.clone(), &TextMark::default());
        ProjNode::Block(BlockNode::Paragraph(lb.finish(k, proj)))
    }

    fn callout(&mut self, c: &Callout, proj: &mut Projection) -> ProjNode {
        let k = self.next_key();
        let mut lb = self.leaf(c.header.clone());
        lb.hidden(c.marker.clone());
        let inl = parse_inline(self.text, c.title.clone());
        lb.inlines(&inl, c.title.clone(), &TextMark::default());
        let title = lb.finish(k, proj);
        let header = self.text[c.header.clone()].to_string();
        let n = self.occurrences.entry(header.clone()).or_insert(0);
        *n += 1;
        let fold_key = format!("{header}\u{0}{n}");
        let mut children = Vec::new();
        for it in &c.body {
            match it {
                QuoteItem::Lines(r) => children.extend(self.body_lines(r, proj)),
                QuoteItem::Callout(_) => children.push(self.item(it, true, proj)),
            }
        }
        let body_start = (c.header.end + 1).min(c.range.end);
        let body: Vec<String> = self.text[body_start..c.range.end]
            .lines()
            .map(|l| {
                let mut l = l;
                for _ in 0..=c.depth {
                    l = l.trim_start_matches([' ', '\t']);
                    l = l.strip_prefix('>').map(|r| r.strip_prefix(' ').unwrap_or(r)).unwrap_or(l);
                }
                l.to_string()
            })
            .collect();
        ProjNode::Callout(CalloutView {
            kind: c.kind.clone(),
            meta: c.meta.clone(),
            fold: c.fold,
            fold_key,
            title,
            title_empty: self.text[c.title.clone()].trim().is_empty(),
            children,
            line: self.text[..c.header.start].matches('\n').count(),
            body,
            body_start,
        })
    }

    /// `in_callout`: body lines of a callout are plain paragraphs; quote lines outside any
    /// callout keep the blockquote style.
    fn item(&mut self, it: &QuoteItem, in_callout: bool, proj: &mut Projection) -> ProjNode {
        match it {
            QuoteItem::Lines(r) => match self.lines(r, proj) {
                ProjNode::Block(b) if !in_callout => ProjNode::Block(BlockNode::Blockquote { children: vec![b], span: None }),
                other => other,
            },
            QuoteItem::Callout(c) => self.callout(c, proj),
        }
    }
}

/// The part of the selection inside a block (revealed markers change its height).
fn sel_signature(sel: &Range<usize>, block: &Range<usize>) -> (usize, usize) {
    if sel.start <= block.end && sel.end >= block.start {
        (sel.start.saturating_sub(block.start), sel.end.min(block.end).saturating_sub(block.start))
    } else {
        (usize::MAX, usize::MAX)
    }
}

/// Obsidian's three ways of showing a note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Rendered, with the construct under the caret revealed.
    #[default]
    LivePreview,
    /// Every construct shows its Markdown.
    Source,
    /// Rendered, nothing revealed, read-only.
    Reading,
}

impl ViewMode {
    /// The id saved in the workspace file (`.tephra/workspace.json`).
    pub fn id(self) -> &'static str {
        match self {
            ViewMode::LivePreview => "live",
            ViewMode::Source => "source",
            ViewMode::Reading => "reading",
        }
    }

    /// The mode an id names; also reads Obsidian's `preview` (its old name for reading view).
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "live" => Some(ViewMode::LivePreview),
            "source" => Some(ViewMode::Source),
            "reading" | "preview" => Some(ViewMode::Reading),
            _ => None,
        }
    }
}

pub struct ProjectionInput<'a> {
    pub text: &'a str,
    pub sel: Selection,
    pub mode: ViewMode,
    pub version: u64,
    pub palette: Palette,
    pub resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub link_label: &'a dyn Fn(&str) -> Option<String>,
    pub code_mode: &'a dyn Fn(&str) -> CodeMode,
    /// Added to every leaf key: 0 for the editor, [`NESTED_KEY_BASE`] for nested Markdown
    /// renders (plugin blocks), so their leaves never collide with the editor's own.
    pub key_base: u64,
}

/// A file that isn't a note (`.css`, `.json`, `.txt`…): its text as one editable leaf, with
/// no Markdown at all (Obsidian renders Markdown only in notes).
pub fn project_plain(text: &str, sel: Selection, version: u64) -> Projection {
    let mut proj = Projection { version, sel, mode: ViewMode::Source, ..Default::default() };
    let k = key(0, 0);
    let mut map = LeafMap { segs: Vec::new(), src: 0..text.len(), len: text.len() };
    if !text.is_empty() {
        map.segs.push(Seg { kind: SegKind::Text, leaf: 0..text.len(), src: 0..text.len() });
    }
    proj.index.insert(k, 0);
    proj.leaves.push((k, map));
    let code = CodeBlock::new(text.to_string().into(), None, None::<Span>).with_leaf_key(k);
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    proj.blocks.push(ProjBlock { node: ProjNode::Block(BlockNode::CodeBlock(code)), indent: 0.0, keys: vec![k], hash: h.finish(), id: k, align: None });
    proj
}

pub fn project(inp: &ProjectionInput) -> Projection {
    let text = inp.text;
    // the reveal rule does the work: reading reveals nothing, source reveals everything
    let sel = match inp.mode {
        ViewMode::LivePreview => inp.sel.range(),
        ViewMode::Source => 0..usize::MAX,
        ViewMode::Reading => usize::MAX..usize::MAX,
    };
    let kb = inp.key_base;
    let mut proj = Projection { version: inp.version, sel: inp.sel, mode: inp.mode, ..Default::default() };
    let blocks = parse_blocks(text);
    let newlines: Vec<usize> = text.bytes().enumerate().filter(|(_, c)| *c == b'\n').map(|(i, _)| i).collect();
    for (bi, b) in blocks.iter().enumerate() {
        let rev = sel.start <= b.range.end && sel.end >= b.range.start;
        let new_leaf = |src: Range<usize>| LeafBuilder::new(text, src, sel.clone(), inp.palette, inp.resolve, inp.link_label);
        let mut nodes: Vec<(ProjNode, Vec<u64>)> = Vec::new();
        let mut indent = 0.0;
        let mut align = None;
        match &b.kind {
            BlockKind::Table if !rev && parse_table(text, b.range.clone()).is_some() => {
                let t = parse_table(text, b.range.clone()).expect("checked");
                let cols = t.rows[0].len();
                let mut keys = Vec::new();
                let mut sub = 0;
                let rows: Vec<Vec<Paragraph>> = t
                    .rows
                    .iter()
                    .map(|row| {
                        (0..cols)
                            .map(|c| match row.get(c) {
                                Some(cell) => {
                                    let k = kb + key(bi, sub);
                                    sub += 1;
                                    let mut lb = new_leaf(cell.clone());
                                    lb.table_cell = true;
                                    let inl = parse_inline(text, cell.clone());
                                    lb.inlines(&inl, cell.clone(), &TextMark::default());
                                    keys.push(k);
                                    lb.finish(k, &mut proj)
                                }
                                None => Paragraph::default(),
                            })
                            .collect()
                    })
                    .collect();
                let aligns = t.aligns.iter().map(|a| column_align(*a)).collect();
                nodes.push((ProjNode::Block(BlockNode::Table(Table::new(rows, aligns))), keys));
            }
            BlockKind::Blank | BlockKind::Paragraph | BlockKind::Table | BlockKind::Frontmatter => {
                let k = kb + key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                if b.kind == BlockKind::Frontmatter {
                    lb.text(b.range.clone(), &TextMark::default(), true);
                } else {
                    let inl = parse_inline(text, b.range.clone());
                    align = block_align(text, &b.range, &inl);
                    lb.inlines(&inl, b.range.clone(), &TextMark::default());
                }
                nodes.push((ProjNode::Block(BlockNode::Paragraph(lb.finish(k, &mut proj))), vec![k]));
            }
            BlockKind::Quote => {
                let items = parse_quote(text, b.range.clone());
                let has_callout = items.iter().any(|i| matches!(i, QuoteItem::Callout(_)));
                if rev || !has_callout {
                    // caret inside (or a plain quote): the source lines, `>` hidden except on the caret's line
                    let k = kb + key(bi, 0);
                    let mut lb = new_leaf(b.range.clone());
                    lb.prefixes = quote_prefixes(text, &b.range, &sel);
                    let inl = parse_inline(text, b.range.clone());
                    lb.inlines(&inl, b.range.clone(), &TextMark::default());
                    let para = lb.finish(k, &mut proj);
                    nodes.push((ProjNode::Block(BlockNode::Blockquote { children: vec![BlockNode::Paragraph(para)], span: None }), vec![k]));
                } else {
                    let mut ctx = QuoteCtx { text, sel: sel.clone(), palette: inp.palette, resolve: inp.resolve, link_label: inp.link_label, bi, sub: 0, key_base: kb, keys: Vec::new(), occurrences: HashMap::new(), newlines: &newlines };
                    for item in &items {
                        let node = ctx.item(item, false, &mut proj);
                        nodes.push((node, std::mem::take(&mut ctx.keys)));
                    }
                }
            }
            BlockKind::Heading { level } => {
                let k = kb + key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                lb.marker(b.marker.clone(), &TextMark::default(), rev);
                let inl = parse_inline(text, b.content.clone());
                lb.inlines(&inl, b.content.clone(), &TextMark::default());
                nodes.push((ProjNode::Block(BlockNode::Heading { level: *level, children: lb.finish(k, &mut proj), span: None }), vec![k]));
            }
            BlockKind::ListItem { task: Some(done), .. } if inp.mode != ViewMode::Source => {
                // Our own checkbox, not the list renderer's: the line stays an ordinary leaf (caret,
                // clicks, editing), and the source `- [ ] ` shows while the caret is on the item.
                let k = kb + key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                lb.marker(b.marker.clone(), &TextMark::default(), rev);
                let inl = parse_inline(text, b.content.clone());
                let start = lb.map.len;
                let mark = if *done && !rev { TextMark::default().strikethrough() } else { TextMark::default() };
                lb.inlines(&inl, b.content.clone(), &mark);
                if *done && !rev {
                    let end = lb.map.len;
                    lb.muted.push(start..end);
                }
                let para = BlockNode::Paragraph(lb.finish(k, &mut proj));
                indent = b.indent as f32 * 6.0;
                let box_at = text[b.marker.clone()].find('[').map(|i| b.marker.start + i).unwrap_or(b.marker.start);
                let node = if rev { ProjNode::Block(para) } else { ProjNode::Task { checked: *done, at: box_at..box_at + 3, child: para } };
                nodes.push((node, vec![k]));
            }
            BlockKind::ListItem { ordered, task } => {
                let k = kb + key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                // the bullet/number is drawn by the list; the source marker stays hidden
                if inp.mode == ViewMode::Source {
                    lb.text(b.marker.clone(), &TextMark::default(), true);
                } else {
                    lb.hidden(b.marker.clone());
                }
                let inl = parse_inline(text, b.content.clone());
                lb.inlines(&inl, b.content.clone(), &TextMark::default());
                let para = lb.finish(k, &mut proj);
                let start = if *ordered {
                    text[b.marker.clone()].trim().trim_end_matches(['.', ')']).parse::<u32>().ok()
                } else {
                    None
                };
                indent = b.indent as f32 * 6.0;
                if inp.mode == ViewMode::Source {
                    // source mode: the marker is text, so no drawn bullet
                    nodes.push((ProjNode::Block(BlockNode::Paragraph(para)), vec![k]));
                } else {
                nodes.push((
                    ProjNode::Block(BlockNode::List {
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
                    }),
                    vec![k],
                ));
                }
            }
            BlockKind::Code { lang, closed } if !rev && (inp.code_mode)(lang) == CodeMode::Rendered => {
                let _ = closed;
                nodes.push((ProjNode::Rendered { lang: lang.clone(), code: text[b.content.clone()].to_string(), at: b.content.start }, vec![]));
            }
            BlockKind::Code { lang, closed } => {
                let open_line = b.range.start..text[b.range.start..].find('\n').map(|i| b.range.start + i).unwrap_or(b.range.end);
                let open_line = open_line.start..open_line.end - usize::from(text[open_line.clone()].ends_with('\r'));
                let close_line = if *closed { text[..b.range.end].rfind('\n').map(|i| i + 1).unwrap_or(b.range.start)..b.range.end } else { b.range.end..b.range.end };
                let mut keys = Vec::new();
                if rev {
                    let k = kb + key(bi, 1);
                    let mut lb = new_leaf(open_line.clone());
                    lb.text(open_line.clone(), &TextMark::default().code(), true);
                    nodes.push((ProjNode::Block(BlockNode::Paragraph(lb.finish(k, &mut proj))), vec![k]));
                }
                let k = kb + key(bi, 0);
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
                let code = CodeBlock::new(text[b.content.clone()].to_string().into(), code_lang(lang).map(Into::into), None::<Span>).with_leaf_key(k);
                let node = match (inp.code_mode)(lang) {
                    CodeMode::Note(note) => ProjNode::Noted { note, child: BlockNode::CodeBlock(code) },
                    _ => ProjNode::Block(BlockNode::CodeBlock(code)),
                };
                nodes.push((node, keys));
                if rev && *closed {
                    let k = kb + key(bi, 2);
                    let mut lb = new_leaf(close_line.clone());
                    lb.text(close_line.clone(), &TextMark::default().code(), true);
                    nodes.push((ProjNode::Block(BlockNode::Paragraph(lb.finish(k, &mut proj))), vec![k]));
                }
            }
            BlockKind::MathBlock => {
                let tex = text[b.content.clone()].to_string();
                let k = kb + key(bi, 0);
                let mut lb = new_leaf(b.range.clone());
                if rev {
                    lb.pad_empty_lines = true;
                    lb.text(b.range.clone(), &TextMark::default().code(), false);
                    nodes.push((ProjNode::Block(BlockNode::Paragraph(lb.finish(k, &mut proj))), vec![k]));
                    // rendered preview under the source (not editable, no key)
                    let mut preview = Paragraph::default();
                    preview.push(InlineNode::custom(MarkdownNode::new(MATH_NODE, MathAtom { tex, display: true }).text(ATOM)));
                    nodes.push((ProjNode::Display { node: BlockNode::Paragraph(preview), line: line_at(&newlines, b.range.start) }, vec![]));
                } else {
                    lb.math_atom(b.range.clone(), &tex, true);
                    nodes.push((ProjNode::Display { node: BlockNode::Paragraph(lb.finish(k, &mut proj)), line: line_at(&newlines, b.range.start) }, vec![k]));
                }
            }
            BlockKind::Rule => {
                let k = kb + key(bi, 0);
                if rev {
                    let mut lb = new_leaf(b.range.clone());
                    lb.text(b.range.clone(), &TextMark::default(), true);
                    nodes.push((ProjNode::Block(BlockNode::Paragraph(lb.finish(k, &mut proj))), vec![k]));
                } else {
                    nodes.push((ProjNode::Block(BlockNode::HorizontalRule { span: None }), vec![]));
                }
            }
        }
        for (part, (node, keys)) in nodes.into_iter().enumerate() {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (&text[b.range.clone()], rev, part, &sel_signature(&sel, &b.range)).hash(&mut h);
            proj.blocks.push(ProjBlock { node, indent, keys, hash: h.finish(), id: kb + key(bi, part), align });
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
        project(&ProjectionInput { text, sel: Selection::caret(caret), mode: ViewMode::LivePreview, version: 0, palette: Palette { highlight: hsla(0.15, 1., 0.5, 0.4) }, resolve: &none, code_mode: &|_| CodeMode::Plain, link_label: &|_| None, key_base: 0 })
    }

    /// A nested Markdown render (a plugin block's body) gets a `key_base`, so its leaf keys and
    /// block ids never collide with the editor's own — otherwise the nested text overwrites the
    /// editor's leaf geometry and the first line becomes unclickable / the caret jumps on edit.
    #[test]
    fn key_base_offsets_every_leaf_key_and_block_id() {
        let none = |_: &str| None;
        let t = "## H\n\npara with $x$ math\n\n> [!algorithm] A\n> body line\n";
        let base = NESTED_KEY_BASE;
        let p = project(&ProjectionInput { text: t, sel: Selection::caret(0), mode: ViewMode::Reading, version: 0, palette: Palette { highlight: hsla(0., 0., 0., 1.) }, resolve: &none, code_mode: &|_| CodeMode::Plain, link_label: &|_| None, key_base: base });
        assert!(!p.leaves.is_empty());
        for (k, _) in &p.leaves {
            assert!(k >= &base, "leaf key {k} below the base");
        }
        for b in &p.blocks {
            assert!(b.id >= base, "block id {} below the base", b.id);
            assert!(b.keys.iter().all(|k| *k >= base));
        }
        // the same text at base 0 has the same keys minus the base
        let p0 = proj(t, 0);
        let k0: Vec<u64> = p0.leaves.iter().map(|(k, _)| *k).collect();
        let k1: Vec<u64> = p.leaves.iter().map(|(k, _)| *k).collect();
        assert_eq!(k1, k0.iter().map(|k| k + base).collect::<Vec<_>>());
    }

    /// The leaf text the renderer sees (atoms as U+FFFC).
    fn leaf_text(p: &Projection, k: u64) -> String {
        let block = p.blocks.iter().find(|b| b.keys.contains(&k)).unwrap();
        fn node_text(n: &ProjNode, k: u64, out: &mut String) {
            match n {
                ProjNode::Block(b) => para_text(b, out),
                ProjNode::Callout(c) => {
                    let mut t = String::new();
                    for ch in c.title.children() {
                        t.push_str(if ch.as_custom().is_some() { ATOM } else { ch.text() });
                    }
                    out.push_str(&t);
                    c.children.iter().for_each(|x| node_text(x, k, out));
                }
                ProjNode::Noted { child, .. } | ProjNode::Display { node: child, .. } | ProjNode::Task { child, .. } => para_text(child, out),
                ProjNode::Rendered { .. } => {}
                ProjNode::Stack(items) => items.iter().for_each(|(_, x)| node_text(x, k, out)),
            }
        }
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
        node_text(&block.node, k, &mut s);
        s
    }

    #[test]
    fn tables_inside_callouts() {
        let t = "> [!table] Costs\n> | a | b |\n> |---|---|\n> | 1 | 2 |\n\nafter\n";
        let p = proj(t, t.len());
        let ProjNode::Callout(c) = &p.blocks[0].node else { panic!("callout") };
        assert!(c.children.iter().any(|n| matches!(n, ProjNode::Block(BlockNode::Table(_)))), "rendered as a table");
        // caret in the table (so in the callout): the callout's source, as for any callout
        let p = proj(t, t.find("| 1").unwrap() + 2);
        assert!(!matches!(p.blocks[0].node, ProjNode::Callout(_)));
    }

    #[test]
    fn callout_tables_follow_gfm() {
        let tables = |t: &str| {
            let p = proj(t, t.len());
            let ProjNode::Callout(c) = &p.blocks[0].node else { panic!("callout") };
            let n = c.children.iter().filter(|n| matches!(n, ProjNode::Block(BlockNode::Table(_)))).count();
            let cells: Vec<String> = p.leaves.iter().map(|(_, m)| t[m.src.clone()].to_string()).collect();
            (n, cells)
        };
        // a quoted text line right below the table is a row; the table interrupts a paragraph
        let (n, cells) = tables("> [!note] N\n> intro\n> | a | b |\n> |---|---|\n> after\n\nz");
        assert_eq!(n, 1);
        assert!(cells.iter().any(|c| c == "after"), "{cells:?}");
        // header/delimiter cell counts differ: not a table
        assert_eq!(tables("> [!note] N\n> | a | b |\n> |---|\n\nz").0, 0);
    }

    #[test]
    fn lists_inside_callouts_are_lists() {
        let t = "> [!note] N\n> intro\n> - one\n> \t- nested\n> 1. first\n> - [x] done\n> after\n\nend\n";
        let p = proj(t, t.len());
        let ProjNode::Callout(c) = &p.blocks[0].node else { panic!("callout") };
        let [ProjNode::Stack(items)] = c.children.as_slice() else { panic!("one run of lines: {}", c.children.len()) };
        let kinds: Vec<(f32, &str)> = items
            .iter()
            .map(|(i, n)| {
                (*i, match n {
                    ProjNode::Block(BlockNode::List { ordered: false, .. }) => "bullet",
                    ProjNode::Block(BlockNode::List { ordered: true, .. }) => "number",
                    ProjNode::Task { checked: true, .. } => "done",
                    ProjNode::Block(BlockNode::Paragraph(_)) => "text",
                    _ => "other",
                })
            })
            .collect();
        assert_eq!(kinds, vec![(0.0, "text"), (0.0, "bullet"), (24.0, "bullet"), (0.0, "number"), (0.0, "done"), (0.0, "text")]);
        let ProjNode::Task { at, .. } = &items[4].1 else { panic!() };
        assert_eq!(&t[at.clone()], "[x]", "the checkbox toggles the source");
        // what the callout shows: no quote prefixes, list markers or `[x]`
        assert_eq!(leaf_text(&p, p.blocks[0].keys[0]), "Nintroonenestedfirstdoneafter");
    }

    #[test]
    fn tasks_get_a_checkbox_until_the_caret_is_on_them() {
        let t = "intro\n- [ ] da fare\n- [x] fatto\n";
        let p = proj(t, 0);
        let tasks: Vec<(bool, &str)> = p.blocks.iter().filter_map(|b| match &b.node {
            ProjNode::Task { checked, at, .. } => Some((*checked, &t[at.clone()])),
            _ => None,
        }).collect();
        assert_eq!(tasks, vec![(false, "[ ]"), (true, "[x]")]);
        let k = p.blocks[1].keys[0];
        assert_eq!(leaf_text(&p, k), "da fare", "marker hidden");
        // caret at the start of the first task's text: its source shows, the other keeps its box
        let p = proj(t, t.find("da").unwrap());
        assert!(matches!(p.blocks[1].node, ProjNode::Block(_)));
        assert_eq!(leaf_text(&p, p.blocks[1].keys[0]), "- [ ] da fare");
        assert!(matches!(p.blocks[2].node, ProjNode::Task { checked: true, .. }));
        // every offset on the task line maps to a caret position
        for off in t.find("- [ ]").unwrap()..t.find("\n- [x]").unwrap() {
            assert!(p.locate(off).is_some(), "offset {off}");
        }
    }

    #[test]
    fn toolbar_html_is_rendered_until_revealed() {
        let t = "a <u>b</u> <font color=\"#ff0000\">c</font> x<sup>2</sup>\n\n<center>mid</center>\n";
        let p = proj(t, t.len());
        let (k, _) = &p.leaves[0];
        assert_eq!(leaf_text(&p, *k), format!("a b c x{ATOM}"), "tags hidden, sup is an atom");
        let colors = &p.colors[k];
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].0, 4..5, "only the font's text is coloured");
        assert_eq!(gpui_kit::Rgba::from(colors[0].1).r, 1.0);
        assert_eq!(p.blocks.iter().filter_map(|b| b.align).collect::<Vec<_>>(), vec![BlockAlign::Center]);
        // caret in an element: its tags show (muted); the others stay rendered
        let p = proj(t, 3);
        assert_eq!(leaf_text(&p, p.leaves[0].0), format!("a <u>b</u> c x{ATOM}"));
        let p = proj(t, t.find("2<").unwrap());
        assert_eq!(leaf_text(&p, p.leaves[0].0), "a b c x<sup>2</sup>");
        // a paragraph only partly inside <center> is not centred
        let p = proj("<center>a</center> b\n", 99);
        assert!(p.blocks.iter().all(|b| b.align.is_none()));
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
        // rendered callout: title leaf "T" (marker hidden), body leaf = the display formula
        assert_eq!(s, "T\u{FFFC}");
        assert!(matches!(p.blocks[0].node, ProjNode::Callout(ref c) if c.kind == "thm" && !c.title_empty));
        let p = proj(t, 3);
        assert!(leaf_text(&p, p.leaves[0].0).starts_with("> [!thm] T\n"));
    }

    #[test]
    fn display_math_in_callouts_is_its_own_block() {
        let t = "> [!note] t\n> line\n> $$a^2$$\n> after\n\nend";
        let p = proj(t, t.len());
        let ProjNode::Callout(c) = &p.blocks[0].node else { panic!() };
        assert_eq!(c.children.len(), 3);
        assert!(matches!(c.children[1], ProjNode::Display { .. }));
        let p = proj(t, t.find("a^2").unwrap()); // caret in the callout: source, no split
        assert!(matches!(p.blocks[0].node, ProjNode::Block(_)) || matches!(&p.blocks[0].node, ProjNode::Callout(c) if c.children.iter().all(|n| !matches!(n, ProjNode::Display { .. }))));
    }

    #[test]
    fn empty_lines_in_revealed_math_are_padded() {
        let t = "$$\n\n$$\n\nx";
        let p = proj(t, 3); // caret on the empty line inside the block
        let (k, m) = &p.leaves[0];
        assert_eq!(leaf_text(&p, *k), format!("$$\n{PAD}\n$$"));
        assert_eq!(m.to_src(3), 3);
        assert_eq!(m.to_leaf(3), 3, "caret before the pad");
        assert_eq!(m.to_src(3 + PAD.len()), 3);
        assert_eq!(m.to_src(3 + PAD.len() + 1), 4);
        // inside a callout: the quote prefix is hidden, the line after it is empty
        let t = "> $$\n> \n> $$\n\nx";
        let p = proj(t, 8);
        let lt = leaf_text(&p, p.leaves[0].0);
        assert!(lt.contains(&format!("\n{PAD}\n")), "{lt:?}");
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
    fn unresolved_wikilinks_are_dimmed() {
        let t = "[[Esiste]] e [[Manca]]\n\nx";
        let resolve = |target: &str| (target == "Esiste").then(|| PathBuf::from("/v/Esiste.md"));
        let p = project(&ProjectionInput { text: t, sel: Selection::caret(t.len()), mode: ViewMode::LivePreview, version: 0, palette: Palette { highlight: gpui_kit::hsla(0., 0., 0., 1.) }, resolve: &resolve, code_mode: &|_| CodeMode::Plain, link_label: &|_| None, key_base: 0 });
        assert_eq!(p.muted[&p.leaves[0].0], vec![9..14]); // "Esiste e Manca": only "Manca"
    }

    #[test]
    fn table_cells_are_leaves() {
        let t = "| a | $x\\|y$ |\n|:-|--:|\n| **1** | 2 |\n\nz";
        let p = proj(t, t.len());
        assert!(matches!(p.blocks[0].node, ProjNode::Block(BlockNode::Table(_))));
        let cells: Vec<(String, std::ops::Range<usize>)> = p.leaves.iter().take(4).map(|(_, m)| (t[m.src.clone()].to_string(), m.src.clone())).collect();
        assert_eq!(cells.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["a", "$x\\|y$", "**1**", "2"]);
        // a cell's leaf maps back to its own source bytes
        let (_, m) = &p.leaves[2];
        assert_eq!(m.to_src(0), t.find("1").unwrap());
        // caret inside the table: source text again
        let p = proj(t, 2);
        assert!(matches!(p.blocks[0].node, ProjNode::Block(BlockNode::Paragraph(_))));
    }

    #[test]
    fn nested_callouts_and_fold() {
        let t = "> [!thm]- T\n> testo\n> > [!pf]\n> > dim\n\nz";
        let p = proj(t, t.len());
        let ProjNode::Callout(c) = &p.blocks[0].node else { panic!() };
        assert_eq!((c.kind.as_str(), c.fold, c.title_empty), ("thm", Some('-'), false));
        assert_eq!(c.children.len(), 2);
        let ProjNode::Callout(pf) = &c.children[1] else { panic!() };
        assert_eq!((pf.kind.as_str(), pf.title_empty), ("pf", true));
        assert_eq!(capitalize(&pf.kind), "Pf");
    }

    #[test]
    fn source_and_reading_modes() {
        let none = |_: &str| None;
        let t = "# H\n- **b** $x$\n> [!thm] T\n> c";
        let mk = |mode| project(&ProjectionInput { text: t, sel: Selection::caret(0), mode, version: 0, palette: Palette { highlight: hsla(0., 0., 0., 1.) }, resolve: &none, code_mode: &|_| CodeMode::Plain, link_label: &|_| None, key_base: 0 });
        let src = mk(ViewMode::Source);
        assert_eq!(leaf_text(&src, src.leaves[0].0), "# H");
        assert_eq!(leaf_text(&src, src.leaves[1].0), "- **b** $x$");
        let read = mk(ViewMode::Reading);
        assert_eq!(leaf_text(&read, read.leaves[0].0), "H"); // caret on the heading, but nothing revealed
        assert!(matches!(read.blocks[2].node, ProjNode::Callout(_)));
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
