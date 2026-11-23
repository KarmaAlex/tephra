//! Inline tokenizer for Obsidian Markdown. Every node has its full source `range` and an
//! `inner` range; the parts of `range` outside `inner` are the syntax markers that live
//! preview hides (e.g. `**` … `**`, `[[target|` … `]]`).

use std::ops::Range;

use super::math::{code_span_end, find_math};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineKind {
    Strong,
    Emph,
    Highlight,
    Strike,
    /// `` `code` ``; inner excludes the backticks.
    Code,
    Math { display: bool },
    /// `[[target#sub|alias]]`; `embed` for `![[…]]`. inner = alias if present, else target.
    WikiLink { embed: bool },
    /// `[text](url)`; inner = text.
    Link,
    /// `![alt](url)`; inner = alt.
    Image,
    /// `\*`: inner = the escaped character.
    Escape,
    /// `%%comment%%`: inner = range (shown dimmed, not hidden).
    Comment,
    /// `#tag`: inner = range.
    Tag,
    /// `^block-id` at the end of a line: inner is empty (hidden unless revealed).
    BlockId,
    /// Inline HTML the editing toolbar writes (`<u>…</u>`, `<font color=…>`, `<sup>`…): the
    /// opening tag is `range.start..inner.start`, the closing tag `inner.end..range.end`.
    Html,
}

/// HTML elements rendered inline (formatting, colours, alignment); others stay text.
pub const HTML_TAGS: &[&str] = &["u", "ins", "sup", "sub", "font", "span", "mark", "b", "strong", "i", "em", "s", "del", "strike", "center", "p", "div"];

/// Tag name of an opening tag (`<font color="red">` → `font`), lowercased.
pub fn html_tag_name(open: &str) -> String {
    open.trim_start_matches('<').split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or("").to_ascii_lowercase()
}

/// Value of attribute `name` in an opening tag (quoted or bare).
pub fn html_attr(open: &str, name: &str) -> Option<String> {
    let lower = open.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = lower[..at].ends_with(|c: char| c.is_whitespace());
        let rest = lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let vstart = open.len() - rest.len() + 1;
        let v = open[vstart..].trim_start();
        let val = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
            _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(val.to_string());
    }
    None
}

/// Paired HTML elements in `s` (relative offsets), skipping text inside `atoms`.
fn html_spans(s: &str, base: usize, atoms: &[Inline]) -> Vec<Inline> {
    let inside_atom = |i: usize| atoms.iter().any(|a| a.range.start - base <= i && i < a.range.end - base);
    // every tag: (start, end, name, closing)
    let mut tags: Vec<(usize, usize, String, bool)> = Vec::new();
    let mut i = 0;
    while let Some(j) = s[i..].find('<') {
        let at = i + j;
        i = at + 1;
        if inside_atom(at) {
            continue;
        }
        let Some(close) = s[at..].find('>').map(|k| at + k + 1) else { break };
        let tag = &s[at..close];
        if tag.contains('\n') {
            continue;
        }
        let closing = tag.starts_with("</");
        let name = html_tag_name(tag.trim_start_matches("</"));
        if HTML_TAGS.contains(&name.as_str()) && !tag.ends_with("/>") {
            tags.push((at, close, name, closing));
            i = close;
        }
    }
    let mut out = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (ti, (start, end, name, closing)) in tags.iter().enumerate() {
        if !closing {
            open.push(ti);
        } else if let Some(pos) = open.iter().rposition(|&o| tags[o].2 == *name) {
            let o = open[pos];
            open.truncate(pos);
            out.push(Inline::leaf(InlineKind::Html, base + tags[o].0..base + end, base + tags[o].1..base + start));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub kind: InlineKind,
    pub range: Range<usize>,
    pub inner: Range<usize>,
    pub children: Vec<Inline>,
}

impl Inline {
    fn leaf(kind: InlineKind, range: Range<usize>, inner: Range<usize>) -> Self {
        Self { kind, range, inner, children: Vec::new() }
    }
    /// Rendered as one indivisible object (formula, image).
    pub fn is_atom(&self) -> bool {
        matches!(self.kind, InlineKind::Math { .. } | InlineKind::Image | InlineKind::WikiLink { embed: true })
    }
    /// Link target text for wikilinks/embeds (`target#sub`, without the alias).
    pub fn link_target<'a>(&self, text: &'a str) -> &'a str {
        match self.kind {
            InlineKind::WikiLink { embed } => {
                let open = if embed { 3 } else { 2 };
                let t = &text[self.range.start + open..self.range.end - 2];
                t.split('|').next().unwrap_or(t)
            }
            InlineKind::Link | InlineKind::Image => {
                let s = &text[self.inner.end..self.range.end];
                s.trim_start_matches(['(', ']']).trim_end_matches(')').split_whitespace().next().unwrap_or("")
            }
            _ => "",
        }
    }
}

fn char_before(s: &str, i: usize) -> Option<char> {
    s[..i].chars().next_back()
}
fn char_at(s: &str, i: usize) -> Option<char> {
    s[i..].chars().next()
}

/// Parses `text[range]`. Offsets in the result are absolute (into `text`).
pub fn parse_inline(text: &str, range: Range<usize>) -> Vec<Inline> {
    let s = &text[range.clone()];
    let base = range.start;
    let b = s.as_bytes();
    let maths = find_math(s);
    let mut mi = 0;
    let mut atoms: Vec<Inline> = Vec::new();
    let line_end = |from: usize| s[from..].find('\n').map(|x| x + from).unwrap_or(s.len());
    let mut i = 0;
    while i < b.len() {
        while mi < maths.len() && maths[mi].range.start < i {
            mi += 1;
        }
        if let Some(m) = maths.get(mi).filter(|m| m.range.start == i) {
            let inner = m.inner();
            atoms.push(Inline::leaf(InlineKind::Math { display: m.display }, base + m.range.start..base + m.range.end, base + inner.start..base + inner.end));
            i = m.range.end;
            continue;
        }
        let rest = &s[i..];
        match b[i] {
            b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => {
                atoms.push(Inline::leaf(InlineKind::Escape, base + i..base + i + 2, base + i + 1..base + i + 2));
                i += 2;
                continue;
            }
            b'`' => {
                let n = rest.bytes().take_while(|&x| x == b'`').count();
                if let Some(end) = code_span_end(b, i) {
                    atoms.push(Inline::leaf(InlineKind::Code, base + i..base + end, base + i + n..base + end - n));
                    i = end;
                } else {
                    i += n;
                }
                continue;
            }
            b'%' if rest.starts_with("%%") => {
                if let Some(j) = s[i + 2..].find("%%") {
                    let end = i + 2 + j + 2;
                    atoms.push(Inline::leaf(InlineKind::Comment, base + i..base + end, base + i..base + end));
                    i = end;
                    continue;
                }
            }
            b'!' if rest.starts_with("![[") => {
                let le = line_end(i);
                if let Some(j) = s[i + 3..le].find("]]").filter(|&j| j > 0) {
                    let end = i + 3 + j + 2;
                    atoms.push(Inline::leaf(InlineKind::WikiLink { embed: true }, base + i..base + end, base + i + 3..base + end - 2));
                    i = end;
                    continue;
                }
            }
            b'!' if rest.starts_with("![") => {
                if let Some((text_end, end)) = md_link(s, i + 1) {
                    atoms.push(Inline::leaf(InlineKind::Image, base + i..base + end, base + i + 2..base + text_end));
                    i = end;
                    continue;
                }
            }
            b'[' if rest.starts_with("[[") => {
                let le = line_end(i);
                if let Some(j) = s[i + 2..le].find("]]").filter(|&j| j > 0) {
                    let end = i + 2 + j + 2;
                    let body = i + 2..end - 2;
                    let inner = match s[body.clone()].find('|') {
                        Some(p) => body.start + p + 1..body.end,
                        None => body,
                    };
                    atoms.push(Inline::leaf(InlineKind::WikiLink { embed: false }, base + i..base + end, base + inner.start..base + inner.end));
                    i = end;
                    continue;
                }
            }
            b'[' => {
                if let Some((text_end, end)) = md_link(s, i) {
                    let mut l = Inline::leaf(InlineKind::Link, base + i..base + end, base + i + 1..base + text_end);
                    l.children = parse_inline(text, l.inner.clone());
                    atoms.push(l);
                    i = end;
                    continue;
                }
            }
            b'#' if char_before(s, i).is_none_or(char::is_whitespace) => {
                let len: usize = s[i + 1..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
                    .map(char::len_utf8)
                    .sum();
                if len > 0 && s[i + 1..i + 1 + len].chars().any(|c| !c.is_ascii_digit()) {
                    atoms.push(Inline::leaf(InlineKind::Tag, base + i..base + i + 1 + len, base + i..base + i + 1 + len));
                    i += 1 + len;
                    continue;
                }
            }
            // `^id` at the end of a line, after whitespace, at line start, or right after a quote `>`
            b'^' if char_before(s, i).is_none_or(|c| c.is_whitespace() || c == '>') => {
                let len = s[i + 1..].bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'-').count();
                let le = line_end(i);
                if len > 0 && s[i + 1 + len..le].trim().is_empty() {
                    atoms.push(Inline::leaf(InlineKind::BlockId, base + i..base + i + 1 + len, base + i + 1 + len..base + i + 1 + len));
                    i += 1 + len;
                    continue;
                }
            }
            _ => {}
        }
        i += char_at(s, i).map(char::len_utf8).unwrap_or(1);
    }
    let spans = emphasis(s, base, &atoms);
    let html = html_spans(s, base, &atoms);
    let mut all = atoms;
    all.extend(spans);
    all.extend(html);
    nest(all)
}

/// `[text](url)` starting at `i` (pointing at `[`): returns (end of text, end of link), relative.
fn md_link(s: &str, i: usize) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut depth = 0;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'\n' if b.get(j + 1) == Some(&b'\n') => return None,
            _ => {}
        }
        j += 1;
    }
    if j >= b.len() || b.get(j + 1) != Some(&b'(') {
        return None;
    }
    let close = s[j + 2..].find(')')? + j + 2;
    if s[j + 2..close].contains('\n') {
        return None;
    }
    Some((j, close + 1))
}

struct Run {
    ch: u8,
    /// Remaining unmatched delimiters: openers consume from the end, closers from the start.
    start: usize,
    end: usize,
    can_open: bool,
    can_close: bool,
}

/// Emphasis/strong/highlight/strike spans over the text outside atoms.
fn emphasis(s: &str, base: usize, atoms: &[Inline]) -> Vec<Inline> {
    let b = s.as_bytes();
    let mut runs: Vec<Run> = Vec::new();
    let mut ai = 0;
    let mut i = 0;
    while i < b.len() {
        while ai < atoms.len() && atoms[ai].range.end - base <= i {
            ai += 1;
        }
        if let Some(a) = atoms.get(ai).filter(|a| a.range.start - base <= i) {
            i = a.range.end - base;
            continue;
        }
        let c = b[i];
        if matches!(c, b'*' | b'_' | b'=' | b'~') {
            let mut n = b[i..].iter().take_while(|&&x| x == c).count();
            // a run stops at the next atom
            if let Some(a) = atoms.get(ai) {
                n = n.min(a.range.start - base - i);
            }
            let prev = char_before(s, i);
            let next = char_at(s, i + n);
            let left = next.is_some_and(|c| !c.is_whitespace());
            let right = prev.is_some_and(|c| !c.is_whitespace());
            let (o, cl) = match c {
                b'_' => (left && !prev.is_some_and(char::is_alphanumeric), right && !next.is_some_and(char::is_alphanumeric)),
                b'=' | b'~' if n != 2 => (false, false),
                _ => (left, right),
            };
            runs.push(Run { ch: c, start: i, end: i + n, can_open: o, can_close: cl });
            i += n;
            continue;
        }
        i += char_at(s, i).map(char::len_utf8).unwrap_or(1);
    }
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for r in 0..runs.len() {
        if runs[r].can_close {
            while runs[r].start < runs[r].end {
                let Some(pos) = stack.iter().rposition(|&o| runs[o].ch == runs[r].ch && runs[o].start < runs[o].end) else { break };
                let o = stack[pos];
                let ch = runs[r].ch;
                let n = match ch {
                    b'=' | b'~' => 2,
                    _ if runs[o].end - runs[o].start >= 2 && runs[r].end - runs[r].start >= 2 => 2,
                    _ => 1,
                };
                let open = runs[o].end - n..runs[o].end;
                let close = runs[r].start..runs[r].start + n;
                if open.end >= close.start {
                    break;
                }
                let kind = match (ch, n) {
                    (b'=', _) => InlineKind::Highlight,
                    (b'~', _) => InlineKind::Strike,
                    (_, 2) => InlineKind::Strong,
                    _ => InlineKind::Emph,
                };
                out.push(Inline::leaf(kind, base + open.start..base + close.end, base + open.end..base + close.start));
                runs[o].end -= n;
                runs[r].start += n;
                stack.truncate(pos + 1); // unmatched openers in between can no longer match
                if runs[o].start == runs[o].end {
                    stack.pop();
                }
            }
        }
        if runs[r].can_open && runs[r].start < runs[r].end {
            stack.push(r);
        }
    }
    out
}

/// Sorts nodes and nests them by containment.
fn nest(mut items: Vec<Inline>) -> Vec<Inline> {
    items.sort_by(|a, b| a.range.start.cmp(&b.range.start).then(b.range.end.cmp(&a.range.end)));
    fn build(items: &mut std::iter::Peekable<std::vec::IntoIter<Inline>>, limit: usize) -> Vec<Inline> {
        let mut out = Vec::new();
        while let Some(next) = items.peek() {
            if next.range.start >= limit {
                break;
            }
            let mut n = items.next().expect("peeked");
            let inner_children = build(items, n.range.end);
            n.children.extend(inner_children);
            n.children.sort_by_key(|c| c.range.start);
            out.push(n);
        }
        out
    }
    let mut it = items.into_iter().peekable();
    build(&mut it, usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use InlineKind::*;

    fn p(s: &str) -> Vec<(InlineKind, &str, &str)> {
        fn walk<'a>(s: &'a str, v: &[Inline], out: &mut Vec<(InlineKind, &'a str, &'a str)>) {
            for n in v {
                out.push((n.kind.clone(), &s[n.range.clone()], &s[n.inner.clone()]));
                walk(s, &n.children, out);
            }
        }
        let mut out = Vec::new();
        walk(s, &parse_inline(s, 0..s.len()), &mut out);
        out
    }

    #[test]
    fn emphasis_kinds() {
        assert_eq!(p("a **b** c"), [(Strong, "**b**", "b")]);
        assert_eq!(p("*it* and _it_"), [(Emph, "*it*", "it"), (Emph, "_it_", "it")]);
        assert_eq!(p("==hl== ~~st~~"), [(Highlight, "==hl==", "hl"), (Strike, "~~st~~", "st")]);
        assert_eq!(p("***both***"), [(Emph, "***both***", "**both**"), (Strong, "**both**", "both")]);
        assert_eq!(p("**a *b* c**"), [(Strong, "**a *b* c**", "a *b* c"), (Emph, "*b*", "b")]);
    }

    #[test]
    fn html_formatting_spans() {
        assert_eq!(p("a <u>word</u> b"), [(Html, "<u>word</u>", "word")]);
        assert_eq!(p("<font color=\"#ff0000\">**red**</font>"), [(Html, "<font color=\"#ff0000\">**red**</font>", "**red**"), (Strong, "**red**", "red")]);
        assert_eq!(p("<sup><sub>x</sub></sup>"), [(Html, "<sup><sub>x</sub></sup>", "<sub>x</sub>"), (Html, "<sub>x</sub>", "x")]);
        // unknown tags, unclosed tags, comparisons and tags inside code stay text
        assert_eq!(p("<b>open"), []);
        assert_eq!(p("a < b > c"), []);
        assert_eq!(p("<blink>x</blink>"), []);
        assert_eq!(p("`<u>x</u>`"), [(Code, "`<u>x</u>`", "<u>x</u>")]);
        assert_eq!(html_attr("<p align=\"center\">", "align").as_deref(), Some("center"));
        assert_eq!(html_attr("<span style='background:#ff0'>", "style").as_deref(), Some("background:#ff0"));
        assert_eq!(html_attr("<font color=red>", "color").as_deref(), Some("red"));
        assert_eq!(html_tag_name("<FONT color=red>"), "font");
    }

    #[test]
    fn non_emphasis() {
        assert_eq!(p("snake_case_name"), []);
        assert_eq!(p("a * b * c"), []);
        assert_eq!(p("x === y"), []);
        assert_eq!(p("**unclosed"), []);
        assert_eq!(p("2*3*4"), [(Emph, "*3*", "3")]); // as in CommonMark
    }

    #[test]
    fn atoms_block_emphasis() {
        assert_eq!(p("`*a*` *b*"), [(Code, "`*a*`", "*a*"), (Emph, "*b*", "b")]);
        assert_eq!(p("$a*b*c$"), [(Math { display: false }, "$a*b*c$", "a*b*c")]);
        assert_eq!(p("**$x$**"), [(Strong, "**$x$**", "$x$"), (Math { display: false }, "$x$", "x")]);
    }

    #[test]
    fn links_and_embeds() {
        assert_eq!(p("![[Pasted image 20250101.png]]"), [(WikiLink { embed: true }, "![[Pasted image 20250101.png]]", "Pasted image 20250101.png")]);
        assert_eq!(p("[[Borůvka#^id|alias]]"), [(WikiLink { embed: false }, "[[Borůvka#^id|alias]]", "alias")]);
        assert_eq!(p("[[Nota]]"), [(WikiLink { embed: false }, "[[Nota]]", "Nota")]);
        assert_eq!(p("[**t**](https://x.y)"), [(Link, "[**t**](https://x.y)", "**t**"), (Strong, "**t**", "t")]);
        assert_eq!(p("![alt](a.png)"), [(Image, "![alt](a.png)", "alt")]);
        let s = "[[a/b#h|c]] ![[d.png]]";
        let v = parse_inline(s, 0..s.len());
        assert_eq!(v[0].link_target(s), "a/b#h");
        assert_eq!(v[1].link_target(s), "d.png");
    }

    #[test]
    fn misc() {
        assert_eq!(p("\\*no\\*"), [(Escape, "\\*", "*"), (Escape, "\\*", "*")]);
        assert_eq!(p("x %%nascosto%% y"), [(Comment, "%%nascosto%%", "%%nascosto%%")]);
        assert_eq!(p("#tag e #123 e c#"), [(Tag, "#tag", "#tag")]);
        assert_eq!(p("Lemma 1 ^lemma1"), [(BlockId, "^lemma1", "")]);
        assert_eq!(p("^lemma1"), [(BlockId, "^lemma1", "")]);
        assert_eq!(p(">^clm1"), [(BlockId, "^clm1", "")]);
        assert_eq!(p("2^n"), []);
    }

    #[test]
    fn offsets_are_absolute_and_unicode_safe() {
        let s = "è **ù** $\\mathbb{N}$";
        let v = parse_inline(s, 3..s.len());
        assert_eq!(&s[v[0].range.clone()], "**ù**");
        assert_eq!(&s[v[1].inner.clone()], "\\mathbb{N}");
    }
}
