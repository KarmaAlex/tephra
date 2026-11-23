//! Fuzzy matching for the quick switcher and the command palette: every word of the query must
//! appear in the text (ignoring case), at a word start ideally; failing that, its characters in
//! order. Returns a cost (lower is better) and the byte ranges to highlight.

use std::ops::Range;

use obsidilight_core::search::fold;

fn word_start(t: &str, i: usize) -> bool {
    i == 0 || t[..i].chars().next_back().is_some_and(|c| !c.is_alphanumeric())
}

/// Cost of `query` against `text` (None = no match) and the matched ranges, sorted.
pub fn score(query: &str, text: &str) -> Option<(u32, Vec<Range<usize>>)> {
    let t = fold(text);
    let mut cost = 0u32;
    let mut ranges = Vec::new();
    for w in fold(query).split_whitespace() {
        // best occurrence: at the very start, then at a word start, then anywhere
        let mut best: Option<(u32, usize)> = None;
        for (i, _) in t.match_indices(w) {
            let c = if i == 0 { 0 } else if word_start(&t, i) { 1 } else { 3 };
            if best.is_none_or(|(b, _)| c < b) {
                best = Some((c, i));
            }
            if c == 0 {
                break;
            }
        }
        match best {
            Some((c, i)) => {
                cost += c;
                ranges.push(i..i + w.len());
            }
            None => {
                // characters in order; the more spread out, the worse
                let mut from = 0;
                let mut first = None;
                for ch in w.chars() {
                    let i = from + t[from..].find(ch)?;
                    first.get_or_insert(i);
                    ranges.push(i..i + ch.len_utf8());
                    from = i + ch.len_utf8();
                }
                let span = from - first.unwrap_or(0);
                cost += 8 + (span - w.len()).min(40) as u32;
            }
        }
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(l) if r.start <= l.end => l.end = l.end.max(r.end),
            _ => merged.push(r),
        }
    }
    Some((cost, merged))
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn ranks_prefix_word_start_substring_then_subsequence() {
        let c = |q: &str, t: &str| score(q, t).map(|s| s.0);
        assert_eq!(c("mst", "MST problem"), Some(0));
        assert_eq!(c("prob", "The MST problem"), Some(1));
        assert_eq!(c("blem", "The MST problem"), Some(3));
        assert_eq!(c("tmp", "The MST problem"), Some(8 + 6), "t..m...p: 6 letters skipped");
        assert!(c("mp", "MST problem") < c("mp", "Maximum independent set problem"), "compact first");
        assert_eq!(c("xyz", "The MST problem"), None);
        assert_eq!(c("", "anything"), Some(0));
        assert_eq!(c("calcolab", "Calcolabilità"), Some(0));
        let (_, r) = score("mst prob", "The MST problem").unwrap();
        assert_eq!(r, vec![4..7, 8..12]);
    }
}
