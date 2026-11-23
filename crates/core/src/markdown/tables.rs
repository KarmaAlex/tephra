//! GFM tables: rows of cell ranges (trimmed), alignment from the delimiter row.
//! Cells split on unescaped `|` (Obsidian requires `\|` inside math in tables, too).

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// First row is the header.
    pub rows: Vec<Vec<Range<usize>>>,
    pub aligns: Vec<Align>,
    /// The `|---|:---:|` line.
    pub delimiter: Range<usize>,
}

fn cells(text: &str, line: Range<usize>) -> Vec<Range<usize>> {
    let s = &text[line.clone()];
    let b = s.as_bytes();
    let mut bars: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'|' => bars.push(i),
            _ => {}
        }
        i += 1;
    }
    let lead = s.len() - s.trim_start().len();
    let trail = s.trim_end().len();
    // boundaries: optional leading and trailing pipes are not cell separators
    let mut bounds = vec![lead];
    let mut seps: Vec<usize> = bars.clone();
    if seps.first() == Some(&lead) {
        seps.remove(0);
        bounds[0] = lead + 1;
    }
    let trailing = seps.last().is_some_and(|&l| l + 1 == trail);
    if trailing {
        seps.pop();
    }
    let end = if trailing { trail - 1 } else { trail };
    let mut out = Vec::new();
    let mut start = bounds[0];
    for &sep in &seps {
        out.push(start..sep);
        start = sep + 1;
    }
    out.push(start..end.max(start));
    out.into_iter()
        .map(|r| {
            let c = &s[r.clone()];
            let a = r.start + (c.len() - c.trim_start().len());
            let e = r.start + c.trim_end().len();
            line.start + a..line.start + e.max(a)
        })
        .collect()
}

fn delimiter_aligns(text: &str, cs: &[Range<usize>]) -> Option<Vec<Align>> {
    cs.iter()
        .map(|r| {
            let c = text[r.clone()].trim();
            let core = c.trim_start_matches(':').trim_end_matches(':');
            if core.is_empty() || !core.chars().all(|ch| ch == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                _ => Align::None,
            })
        })
        .collect()
}

/// Parses the table block `r`; None if its second line is not a delimiter row.
pub fn parse_table(text: &str, r: Range<usize>) -> Option<Table> {
    let mut lines = Vec::new();
    let mut ls = r.start;
    for raw in text[r.clone()].split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        lines.push(ls..ls + line.len());
        ls += raw.len() + 1;
    }
    parse_table_lines(text, &lines)
}

/// Parses a table from its line ranges (without line breaks), e.g. the lines of a table inside
/// a callout with their `> ` prefixes left out. None if the second line is not a delimiter row.
pub fn parse_table_lines(text: &str, lines: &[Range<usize>]) -> Option<Table> {
    if lines.len() < 2 {
        return None;
    }
    let header = cells(text, lines[0].clone());
    let aligns = delimiter_aligns(text, &cells(text, lines[1].clone()))?;
    let mut rows = vec![header];
    for l in &lines[2..] {
        rows.push(cells(text, l.clone()));
    }
    Some(Table { rows, aligns, delimiter: lines[1].clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_alignment_and_escaped_pipes() {
        let t = "| a | b | c |\n|:--|:-:|--:|\n| $x\\|y$ | **è** |  |";
        let tb = parse_table(t, 0..t.len()).unwrap();
        assert_eq!(tb.aligns, vec![Align::Left, Align::Center, Align::Right]);
        let row: Vec<&str> = tb.rows[1].iter().map(|r| &t[r.clone()]).collect();
        assert_eq!(row, vec!["$x\\|y$", "**è**", ""]);
        let head: Vec<&str> = tb.rows[0].iter().map(|r| &t[r.clone()]).collect();
        assert_eq!(head, vec!["a", "b", "c"]);
    }

    #[test]
    fn without_outer_pipes_and_not_a_table() {
        let t = "a | b\n--- | ---\n1 | 2";
        let tb = parse_table(t, 0..t.len()).unwrap();
        assert_eq!(tb.rows[1].iter().map(|r| &t[r.clone()]).collect::<Vec<_>>(), vec!["1", "2"]);
        assert!(parse_table("| a |\n| b |", 0..11).is_none());
    }

    #[test]
    fn quoted_lines() {
        let t = "> | a | b |\n> |---|---|\n> | 1 | 2 |";
        let lines = vec![2..11, 14..23, 26..35];
        let tb = parse_table_lines(t, &lines).unwrap();
        assert_eq!(tb.rows[1].iter().map(|r| &t[r.clone()]).collect::<Vec<_>>(), vec!["1", "2"]);
    }
}
