//! Blockquotes and callouts (`> [!type|meta]± Title`), nested.
//!
//! Obsidian's rule: `[!type]` is a callout only on the first line of a quote at its depth
//! (bench task t5, reference-validated). A quote block is parsed into a tree of plain-line runs
//! and callouts; every range is absolute (into the document text).

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callout {
    /// Lowercased type, e.g. `thm`.
    pub kind: String,
    /// Text after `|` inside the brackets, e.g. `lines=true`.
    pub meta: String,
    /// `+` (expanded, foldable), `-` (collapsed), or none.
    pub fold: Option<char>,
    /// The header line, without its line break.
    pub header: Range<usize>,
    /// Prefix of the header line up to the title: `> > [!thm]- `.
    pub marker: Range<usize>,
    /// Title text (may be empty).
    pub title: Range<usize>,
    pub depth: usize,
    /// Header plus body lines.
    pub range: Range<usize>,
    pub body: Vec<QuoteItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuoteItem {
    /// Consecutive lines belonging directly to the enclosing callout/quote (quote prefixes included).
    Lines(Range<usize>),
    Callout(Callout),
}

#[derive(Clone, Copy, Debug)]
struct Line {
    start: usize,
    end: usize,
    depth: usize,
    /// Offset where the content begins (after the last `>` and one optional space).
    content: usize,
}

fn lines(text: &str, r: &Range<usize>) -> Vec<Line> {
    let mut out = Vec::new();
    let mut ls = r.start;
    for raw in text[r.clone()].split('\n') {
        let le = ls + raw.len();
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let b = line.as_bytes();
        let mut k = 0;
        while k < b.len() && b[k] == b' ' && k < 3 {
            k += 1;
        }
        let (mut depth, mut content) = (0, k);
        while k < b.len() && b[k] == b'>' {
            depth += 1;
            k += 1;
            content = k;
            if k < b.len() && b[k] == b' ' {
                content = k + 1;
            }
            while k < b.len() && b[k] == b' ' {
                k += 1;
            }
        }
        if depth == 0 {
            content = 0;
            // a lazy continuation line (no `>`) stays in the callout the previous line was in
            if !line.trim().is_empty() {
                depth = out.last().map_or(0, |p: &Line| p.depth);
            }
        }
        out.push(Line { start: ls, end: ls + line.len(), depth, content: ls + content });
        ls = le + 1;
    }
    out
}

/// Parses `[!type|meta]±` at the start of `s`: (kind, meta, fold, bytes consumed incl. following spaces).
fn header(s: &str) -> Option<(String, String, Option<char>, usize)> {
    let rest = s.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let inside = &rest[..close];
    if inside.is_empty() {
        return None;
    }
    let (kind, meta) = inside.split_once('|').unwrap_or((inside, ""));
    let mut used = 2 + close + 1;
    let fold = s[used..].chars().next().filter(|c| *c == '+' || *c == '-');
    if fold.is_some() {
        used += 1;
    }
    used += s[used..].len() - s[used..].trim_start_matches([' ', '\t']).len();
    Some((kind.trim().to_lowercase(), meta.trim().to_string(), fold, used))
}

/// Parses the quote block `r` (all lines start with `>`, possibly with lazy continuation lines).
pub fn parse_quote(text: &str, r: Range<usize>) -> Vec<QuoteItem> {
    let ls = lines(text, &r);
    items(text, &ls, 0)
}

fn items(text: &str, ls: &[Line], depth: usize) -> Vec<QuoteItem> {
    let mut out = Vec::new();
    let mut run: Option<Range<usize>> = None;
    let mut i = 0;
    while i < ls.len() {
        let l = ls[i];
        let first_at_depth = i == 0 || ls[i - 1].depth < l.depth;
        let is_header = l.depth > depth && first_at_depth && header(&text[l.content..l.end]).is_some();
        if is_header {
            if let Some(r) = run.take() {
                out.push(QuoteItem::Lines(r));
            }
            let mut j = i + 1;
            while j < ls.len() && ls[j].depth >= l.depth {
                j += 1;
            }
            let (kind, meta, fold, used) = header(&text[l.content..l.end]).expect("checked");
            let body = items(text, &ls[i + 1..j], l.depth);
            out.push(QuoteItem::Callout(Callout {
                kind,
                meta,
                fold,
                header: l.start..l.end,
                marker: l.start..l.content + used,
                title: l.content + used..l.end,
                depth: l.depth,
                range: l.start..ls[j - 1].end,
                body,
            }));
            i = j;
            continue;
        }
        run = Some(match run {
            Some(r) => r.start..l.end,
            None => l.start..l.end,
        });
        i += 1;
    }
    if let Some(r) = run {
        out.push(QuoteItem::Lines(r));
    }
    out
}

/// All callouts in a quote tree, depth-first.
pub fn flatten(items: &[QuoteItem]) -> Vec<&Callout> {
    let mut out = Vec::new();
    for it in items {
        if let QuoteItem::Callout(c) = it {
            out.push(c);
            out.extend(flatten(&c.body));
        }
    }
    out
}

/// Quote-marker prefix ranges (`>`, `> >` plus one space) at the start of each line in `r`.
pub fn quote_prefixes(text: &str, r: &Range<usize>) -> Vec<Range<usize>> {
    lines(text, r).into_iter().filter(|l| l.depth > 0).map(|l| l.start..l.content).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(items: &[QuoteItem]) -> Vec<(String, usize)> {
        flatten(items).into_iter().map(|c| (c.kind.clone(), c.depth)).collect()
    }

    #[test]
    fn simple_with_fold_meta_title() {
        let t = "> [!Algorithm|lines=true]- Dijkstra\n> body";
        let it = parse_quote(t, 0..t.len());
        let QuoteItem::Callout(c) = &it[0] else { panic!() };
        assert_eq!((c.kind.as_str(), c.meta.as_str(), c.fold), ("algorithm", "lines=true", Some('-')));
        assert_eq!(&t[c.title.clone()], "Dijkstra");
        assert_eq!(&t[c.marker.clone()], "> [!Algorithm|lines=true]- ");
        assert_eq!(c.body, vec![QuoteItem::Lines(36..t.len())]);
    }

    #[test]
    fn nested_and_siblings_children() {
        let t = "> [!thm] T\n> text\n> > [!pf] Proof\n> > x\n> back\n> > [!rem]\n> > r";
        let it = parse_quote(t, 0..t.len());
        assert_eq!(kinds(&it), vec![("thm".into(), 1), ("pf".into(), 2), ("rem".into(), 2)]);
        let QuoteItem::Callout(thm) = &it[0] else { panic!() };
        assert_eq!(thm.body.len(), 4); // text, pf, back, rem
        let QuoteItem::Callout(rem) = &thm.body[3] else { panic!() };
        assert_eq!(&t[rem.title.clone()], "");
    }

    #[test]
    fn marker_on_later_line_is_text() {
        let t = "> just a quote\n> [!thm] not a callout";
        assert_eq!(parse_quote(t, 0..t.len()), vec![QuoteItem::Lines(0..t.len())]);
    }

    #[test]
    fn spaced_markers_and_prefixes() {
        let t = "> [!thm] A\n>> [!pf]\n> > x";
        let it = parse_quote(t, 0..t.len());
        assert_eq!(kinds(&it), vec![("thm".into(), 1), ("pf".into(), 2)]);
        let pre: Vec<&str> = quote_prefixes(t, &(0..t.len())).into_iter().map(|r| &t[r]).collect();
        assert_eq!(pre, vec!["> ", ">> ", "> > "]);
    }
}
