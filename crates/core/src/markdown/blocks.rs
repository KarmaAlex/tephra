//! Line-based block splitter for Obsidian Markdown.
//!
//! Every line of the document belongs to exactly one block, blank lines included (each
//! blank line is its own `Blank` block, so the caret can sit on it). Block ranges exclude
//! the line break after their last line.

use std::ops::Range;

use super::tables::is_table_start;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Frontmatter,
    Heading { level: u8 },
    Paragraph,
    ListItem { ordered: bool, task: Option<bool> },
    /// Blockquote, including callouts (`> [!type]`); refined in M1b.
    Quote,
    Code { lang: String, closed: bool },
    MathBlock,
    Table,
    Rule,
    Blank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Whole lines, without the final line break.
    pub range: Range<usize>,
    /// Inline content (heading text, item text, paragraph text), or the inner text of code/math.
    pub content: Range<usize>,
    /// Syntax prefix hidden in live preview: `## `, `- `, `- [ ] `, `1. `. Empty if none.
    pub marker: Range<usize>,
    /// Leading whitespace width (list nesting), tabs counted as 4.
    pub indent: usize,
}

#[derive(Clone, Copy, Debug)]
struct Line {
    start: usize,
    /// End of content, before "\r\n"/"\n".
    end: usize,
}

fn lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            let end = if i > start && text.as_bytes()[i - 1] == b'\r' { i - 1 } else { i };
            out.push(Line { start, end });
            start = i + 1;
        }
    }
    out.push(Line { start, end: text.len() });
    out
}

fn indent_of(s: &str) -> (usize, usize) {
    // (bytes, columns)
    let mut cols = 0;
    let mut bytes = 0;
    for c in s.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
        bytes += 1;
    }
    (bytes, cols)
}

/// Fence opener: (char, run length, lang).
fn fence_open(s: &str) -> Option<(u8, usize, String)> {
    let (ib, ic) = indent_of(s);
    if ic > 3 {
        return None;
    }
    let r = &s[ib..];
    let c = *r.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = r.bytes().take_while(|&x| x == c).count();
    if n < 3 {
        return None;
    }
    let info = r[n..].trim();
    if c == b'`' && info.contains('`') {
        return None;
    }
    Some((c, n, info.split_whitespace().next().unwrap_or("").to_string()))
}

fn fence_close(s: &str, c: u8, n: usize) -> bool {
    let (ib, ic) = indent_of(s);
    if ic > 3 {
        return false;
    }
    let r = &s[ib..];
    let m = r.bytes().take_while(|&x| x == c).count();
    m >= n && r[m..].trim().is_empty()
}

fn heading(s: &str) -> Option<(u8, usize)> {
    // (level, marker length in bytes including following spaces)
    let (ib, ic) = indent_of(s);
    if ic > 3 {
        return None;
    }
    let r = &s[ib..];
    let n = r.bytes().take_while(|&x| x == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = &r[n..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let sp = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    Some((n as u8, ib + n + sp))
}

fn is_rule(s: &str) -> bool {
    let t = s.trim();
    let Some(c) = t.chars().next() else { return false };
    matches!(c, '-' | '*' | '_') && t.chars().all(|x| x == c || x == ' ') && t.chars().filter(|&x| x == c).count() >= 3
}

/// List item: (indent bytes, indent cols, marker length incl. task and spaces, ordered, task).
fn list_item(s: &str) -> Option<(usize, usize, usize, bool, Option<bool>)> {
    let (ib, ic) = indent_of(s);
    let r = &s[ib..];
    let (mlen, ordered) = if r.starts_with(['-', '*', '+']) {
        (1, false)
    } else {
        let d = r.bytes().take_while(u8::is_ascii_digit).count();
        if d == 0 || d > 9 || !matches!(r.as_bytes().get(d), Some(b'.') | Some(b')')) {
            return None;
        }
        (d + 1, true)
    };
    let after = &r[mlen..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    // "---" / "***" are rules, not list items
    if !ordered && is_rule(s) {
        return None;
    }
    let sp = after.len() - after.trim_start_matches([' ', '\t']).len();
    let mut marker = mlen + sp;
    let mut task = None;
    let rest = &r[marker..];
    for (pat, v) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if rest.starts_with(pat) && (rest.len() == 3 || rest[3..].starts_with(' ')) {
            task = Some(v);
            marker += if rest[3..].starts_with(' ') { 4 } else { 3 };
            break;
        }
    }
    Some((ib, ic, ib + marker, ordered, task))
}

fn is_quote(s: &str) -> bool {
    let (ib, ic) = indent_of(s);
    ic <= 3 && s[ib..].starts_with('>')
}
/// GFM: once a table has started, every following line is a row until a blank line or a line
/// that starts another block (so plain text right below a table becomes a row).
pub fn continues_table(s: &str) -> bool {
    !s.trim().is_empty() && !interrupts(s)
}

/// A quote line without its `>` markers.
fn quote_inner(s: &str) -> &str {
    s.trim_start().trim_start_matches(|c: char| c == '>' || c == ' ' || c == '\t')
}
fn starts_math_block(s: &str) -> bool {
    s.trim_start().starts_with("$$")
}

/// Does this line start a block that interrupts a paragraph or list item?
/// CommonMark's lazy continuation: a plain text line right after a quote line that holds
/// paragraph text continues that paragraph, so it stays in the quote (`^block-id` lines too,
/// and pipe lines: a lazy line cannot start a table).
fn lazy_quote_line(prev: &str, next: &str) -> bool {
    let inner = quote_inner(prev);
    !next.trim().is_empty() && !interrupts(next) && !inner.is_empty() && fence_open(inner).is_none() && !inner.starts_with("$$")
}

fn interrupts(s: &str) -> bool {
    heading(s).is_some() || fence_open(s).is_some() || is_quote(s) || starts_math_block(s) || is_rule(s) || list_item(s).is_some()
}

/// For a `$$` opened at byte `open`: (line index, byte offset) of the first unescaped closing `$$`,
/// but only if nothing except whitespace follows it on its line (otherwise the `$$…$$` is inline
/// display math inside a paragraph, e.g. `$$a$$ and $$b$$`).
fn math_block_end(text: &str, ls: &[Line], i: usize, open: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let mut j = open + 2;
    while j + 1 < b.len() {
        if b[j] == b'$' && b[j + 1] == b'$' && !escaped(b, j) {
            let line = ls[i..].iter().position(|l| l.start <= j && j <= l.end).map(|k| k + i)?;
            return text[j + 2..ls[line].end].trim().is_empty().then_some((line, j));
        }
        j += 1;
    }
    None
}

fn escaped(b: &[u8], i: usize) -> bool {
    let mut n = 0;
    while i > n && b[i - 1 - n] == b'\\' {
        n += 1;
    }
    n % 2 == 1
}

/// Does `s` end inside display math (an unmatched `$$` after the last complete formula)?
fn ends_in_open_display(s: &str) -> bool {
    let last = super::math::find_math(s).last().map(|m| m.range.end).unwrap_or(0);
    let b = s.as_bytes();
    (last..b.len().saturating_sub(1)).any(|k| b[k] == b'$' && b[k + 1] == b'$' && !escaped(b, k))
}

pub fn parse_blocks(text: &str) -> Vec<Block> {
    let ls = lines(text);
    let line = |i: usize| &text[ls[i].start..ls[i].end];
    let mut out = Vec::new();
    // GFM: a table starts at a header line followed by a delimiter row with as many cells
    let table_at = |k: usize| k + 1 < ls.len() && is_table_start(line(k), line(k + 1));
    let mut i = 0;
    let push =|out: &mut Vec<Block>, kind, a: usize, b: usize, content: Range<usize>, marker: Range<usize>, indent| {
        out.push(Block { kind, range: ls[a].start..ls[b].end, content, marker, indent });
    };
    while i < ls.len() {
        let s = line(i);
        let st = ls[i].start;
        // frontmatter
        if i == 0 && s == "---" {
            if let Some(j) = (1..ls.len()).find(|&j| line(j) == "---") {
                let content = if j > 1 { ls[1].start..ls[j - 1].end } else { ls[0].end..ls[0].end };
                push(&mut out, BlockKind::Frontmatter, 0, j, content, 0..0, 0);
                i = j + 1;
                continue;
            }
        }
        if s.trim().is_empty() {
            push(&mut out, BlockKind::Blank, i, i, ls[i].start..ls[i].end, st..st, 0);
            i += 1;
            continue;
        }
        if let Some((c, n, lang)) = fence_open(s) {
            let close = (i + 1..ls.len()).find(|&j| fence_close(line(j), c, n));
            let last = close.unwrap_or(ls.len() - 1);
            let content = if last > i + (close.is_some() as usize) {
                ls[i + 1].start..ls[last - (close.is_some() as usize)].end
            } else {
                ls[i].end..ls[i].end
            };
            push(&mut out, BlockKind::Code { lang, closed: close.is_some() }, i, last, content, st..ls[i].end, 0);
            i = last + 1;
            continue;
        }
        if starts_math_block(s) {
            let open = st + s.find("$$").unwrap_or(0);
            if let Some((j, close)) = math_block_end(text, &ls, i, open) {
                let content = if close > open + 2 { open + 2..close } else { open + 2..open + 2 };
                push(&mut out, BlockKind::MathBlock, i, j, content, 0..0, 0);
                i = j + 1;
                continue;
            }
        }
        if let Some((level, m)) = heading(s) {
            push(&mut out, BlockKind::Heading { level }, i, i, st + m..ls[i].end, st..st + m, 0);
            i += 1;
            continue;
        }
        if is_rule(s) {
            push(&mut out, BlockKind::Rule, i, i, ls[i].end..ls[i].end, st..ls[i].end, 0);
            i += 1;
            continue;
        }
        if is_quote(s) {
            // a table inside the quote is not a paragraph, so no lazy line can follow it
            let quoted = |k: usize| k < ls.len() && is_quote(line(k));
            let starts = |k: usize| quoted(k + 1) && is_table_start(quote_inner(line(k)), quote_inner(line(k + 1)));
            let mut in_table = starts(i);
            let mut j = i;
            while j + 1 < ls.len() {
                let n = j + 1;
                if quoted(n) {
                    in_table = (in_table && continues_table(quote_inner(line(n)))) || starts(n);
                } else if ends_in_open_display(&text[st..ls[j].end]) {
                    in_table = false;
                } else if in_table || !lazy_quote_line(line(j), line(n)) {
                    break;
                }
                j = n;
            }
            push(&mut out, BlockKind::Quote, i, j, st..ls[j].end, 0..0, 0);
            i = j + 1;
            continue;
        }
        if list_item(s).is_none() && table_at(i) {
            let mut j = i + 1;
            while j + 1 < ls.len() && continues_table(line(j + 1)) {
                j += 1;
            }
            push(&mut out, BlockKind::Table, i, j, st..ls[j].end, 0..0, 0);
            i = j + 1;
            continue;
        }
        if let Some((_ib, ic, m, ordered, task)) = list_item(s) {
            let mut j = i;
            // lazy / indented continuation lines belong to the item
            while j + 1 < ls.len() {
                let n = line(j + 1);
                // a table interrupts the item only when indented into it (a lazy line cannot start one)
                let table = table_at(j + 1) && indent_of(n).1 > ic + 1;
                if (n.trim().is_empty() || interrupts(n) || table) && !ends_in_open_display(&text[st..ls[j].end]) {
                    break;
                }
                j += 1;
            }
            push(&mut out, BlockKind::ListItem { ordered, task }, i, j, st + m..ls[j].end, st..st + m, ic);
            i = j + 1;
            continue;
        }
        // paragraph
        let mut j = i;
        while j + 1 < ls.len() {
            let n = line(j + 1);
            if (n.trim().is_empty() || interrupts(n) || table_at(j + 1)) && !ends_in_open_display(&text[st..ls[j].end]) {
                break;
            }
            j += 1;
        }
        push(&mut out, BlockKind::Paragraph, i, j, st..ls[j].end, st..st, 0);
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(BlockKind, &str)> {
        parse_blocks(s).into_iter().map(|b| (b.kind, &s[b.range])).collect()
    }

    #[test]
    fn every_line_is_covered_once() {
        let s = "---\ncssclasses: Plain\n---\n# T\n\npara a\npara b\n- x\n  cont\n- [ ] task\n1. one\n\n```c\nint x;\n```\n$$\na\\\\b\n$$\n> [!thm] T\n> x\n| a | b |\n|---|---|\n---\n";
        let bs = parse_blocks(s);
        let mut covered = 0;
        for b in &bs {
            assert!(b.range.start >= covered, "{b:?}");
            covered = b.range.end;
        }
        let line_count = s.split('\n').count();
        let block_lines: usize = bs.iter().map(|b| s[b.range.clone()].split('\n').count()).sum();
        assert_eq!(block_lines, line_count);
    }

    #[test]
    fn kinds_in_order() {
        let s = "---\na: 1\n---\n# Tìtolo\n\npara a\npara b\n- x\n  cont\n- [x] fatto\n2) due\n```c++\nint x;\n```\n$$\nx\n$$\n> q\n\n| a |\n|---|\n***\n";
        let k: Vec<_> = kinds(s).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            k,
            vec![
                BlockKind::Frontmatter,
                BlockKind::Heading { level: 1 },
                BlockKind::Blank,
                BlockKind::Paragraph,
                BlockKind::ListItem { ordered: false, task: None },
                BlockKind::ListItem { ordered: false, task: Some(true) },
                BlockKind::ListItem { ordered: true, task: None },
                BlockKind::Code { lang: "c++".into(), closed: true },
                BlockKind::MathBlock,
                BlockKind::Quote,
                BlockKind::Blank,
                BlockKind::Table,
                BlockKind::Rule,
                BlockKind::Blank,
            ]
        );
    }

    #[test]
    fn markers_and_content() {
        let s = "## Titolo\n- [ ] cosa\n  1. nested";
        let bs = parse_blocks(s);
        assert_eq!(&s[bs[0].marker.clone()], "## ");
        assert_eq!(&s[bs[0].content.clone()], "Titolo");
        assert_eq!(&s[bs[1].marker.clone()], "- [ ] ");
        assert_eq!(&s[bs[1].content.clone()], "cosa");
        assert_eq!(bs[2].indent, 2);
        assert_eq!(&s[bs[2].content.clone()], "nested");
    }

    #[test]
    fn math_block_single_and_multi_line() {
        let s = "$$x^2$$\n$$\n\\begin{cases}a\\\\b\\end{cases}\n$$\n";
        let bs = parse_blocks(s);
        assert_eq!(bs[0].kind, BlockKind::MathBlock);
        assert_eq!(&s[bs[0].content.clone()], "x^2");
        assert_eq!(bs[1].kind, BlockKind::MathBlock);
        assert_eq!(&s[bs[1].content.clone()], "\n\\begin{cases}a\\\\b\\end{cases}\n");
    }

    #[test]
    fn display_math_spans_lazy_lines_and_inline_pairs() {
        let s = "> testo $$\\begin{aligned}\na\\\\b\n$$\n\n$$a$$ e $$b$$\n";
        let k: Vec<_> = kinds(s).into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, vec![BlockKind::Quote, BlockKind::Blank, BlockKind::Paragraph, BlockKind::Blank]);
    }

    #[test]
    fn lazy_lines_continue_a_quote() {
        let k: Vec<_> = kinds("> [!lemma]\n> text\n^lemma1\n\nafter\n").into_iter().map(|(k, s)| (k, s.to_string())).collect();
        assert_eq!(k[0], (BlockKind::Quote, "> [!lemma]\n> text\n^lemma1".to_string()));
        let k: Vec<_> = kinds("> a\n>\nb\n").into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, vec![BlockKind::Quote, BlockKind::Paragraph, BlockKind::Blank], "no lazy line after an empty quote line");
        let k: Vec<_> = kinds("> a\n# H\n").into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, vec![BlockKind::Quote, BlockKind::Heading { level: 1 }, BlockKind::Blank]);
    }

    #[test]
    fn unclosed_fence_runs_to_end() {
        let s = "```\na\n# not heading";
        let bs = parse_blocks(s);
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, BlockKind::Code { lang: String::new(), closed: false });
    }

    #[test]
    fn crlf() {
        let s = "# A\r\n\r\ntext\r\n";
        let bs = parse_blocks(s);
        assert_eq!(&s[bs[0].content.clone()], "A");
        assert_eq!(bs[1].kind, BlockKind::Blank);
        assert_eq!(&s[bs[2].range.clone()], "text");
    }

    #[test]
    fn hashtag_is_not_heading_and_dash_rule_is_not_list() {
        let k: Vec<_> = kinds("#tag\n---\n- - -").into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, vec![BlockKind::Paragraph, BlockKind::Rule, BlockKind::Rule]);
    }

    /// Reference outputs from cmark-gfm 0.29 (`cmark-gfm -e table`).
    #[test]
    fn gfm_table_boundaries() {
        let k = |s: &str| kinds(s).into_iter().map(|(k, s)| (k, s.to_string())).collect::<Vec<_>>();
        let (t, p, b) = (BlockKind::Table, BlockKind::Paragraph, BlockKind::Blank);
        // plain text right below a table is one more row; a blank line ends the table
        assert_eq!(k("| a | b |\n|---|---|\n| 1 | 2 |\nafter\n\nbar"), vec![(t.clone(), "| a | b |\n|---|---|\n| 1 | 2 |\nafter".into()), (b.clone(), "".into()), (p.clone(), "bar".into())]);
        // rows and header need no outer pipes
        assert_eq!(k("a | b\n--- | ---\n1 | 2"), vec![(t.clone(), "a | b\n--- | ---\n1 | 2".into())]);
        assert_eq!(k("| a | b |\n|---|---|\n1 | 2"), vec![(t.clone(), "| a | b |\n|---|---|\n1 | 2".into())]);
        // a table interrupts a paragraph; its header is the line before the delimiter row
        assert_eq!(k("p1\np2\n| a |\n|---|"), vec![(p.clone(), "p1\np2".into()), (t.clone(), "| a |\n|---|".into())]);
        // not tables: cell count mismatch, no delimiter row, setext underline
        assert_eq!(k("| a | b |\n|---|\n| 1 | 2 |"), vec![(p.clone(), "| a | b |\n|---|\n| 1 | 2 |".into())]);
        assert_eq!(k("| a |\n| b |"), vec![(p.clone(), "| a |\n| b |".into())]);
        assert_eq!(k("| a |\n---").into_iter().map(|(k, _)| k).collect::<Vec<_>>(), vec![p.clone(), BlockKind::Rule]);
        // other blocks end a table
        let ends = |s: &str| kinds(s).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(ends("| a |\n|---|\n# H"), vec![t.clone(), BlockKind::Heading { level: 1 }]);
        assert_eq!(ends("| a |\n|---|\n- x"), vec![t.clone(), BlockKind::ListItem { ordered: false, task: None }]);
        assert_eq!(ends("| a |\n|---|\n> q"), vec![t.clone(), BlockKind::Quote]);
        assert_eq!(ends("| a |\n|---|\n***"), vec![t.clone(), BlockKind::Rule]);
        assert_eq!(ends("| a |\n|---|\n```\nx\n```"), vec![t.clone(), BlockKind::Code { lang: String::new(), closed: true }]);
        // a list item line is never a table header
        assert_eq!(ends("- a | b\n--- | ---"), vec![BlockKind::ListItem { ordered: false, task: None }]);
    }

    #[test]
    fn lazy_lines_cannot_start_a_table() {
        let ends = |s: &str| kinds(s).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        assert_eq!(ends("- x\n| a | b |\n|---|---|"), vec![BlockKind::ListItem { ordered: false, task: None }]);
        assert_eq!(ends("- x\n  | a | b |\n  |---|---|"), vec![BlockKind::ListItem { ordered: false, task: None }, BlockKind::Table]);
        assert_eq!(ends("> q\n| a | b |\n|---|---|"), vec![BlockKind::Quote]);
        // a table in a quote takes no lazy lines; a quoted paragraph does
        assert_eq!(ends("> | a | b |\n> |---|---|\n> | 1 | 2 |\nlazy"), vec![BlockKind::Quote, BlockKind::Paragraph]);
        assert_eq!(ends("> q\n> | a |\n> |---|\n>\n> r\nlazy"), vec![BlockKind::Quote]);
    }
}
