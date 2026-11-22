//! What a link's hover preview shows (Obsidian's Page preview, Academic Notes' reference
//! previews): the callout a `#^block` sits in, a heading's section, or the start of the note.

use obsidilight_core::markdown::{callouts::flatten, parse_blocks, parse_quote, BlockKind};

/// Lines of the preview at most.
const MAX_LINES: usize = 40;

/// The source to preview for `sub` (the part after `#`, empty = whole note) in `text`, and the
/// line it starts on.
pub fn snippet(text: &str, sub: &str) -> Option<(String, usize)> {
    let line_of = |off: usize| text[..off].matches('\n').count();
    let cap = |s: &str| s.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    let blocks = parse_blocks(text);
    if let Some(id) = sub.strip_prefix('^') {
        let pat = format!("^{id}");
        let mut off = 0;
        let mut at = None;
        for l in text.split('\n') {
            let t = l.trim_end();
            if t.ends_with(&pat) && t[..t.len() - pat.len()].chars().next_back().is_none_or(|c| c.is_whitespace() || c == '>') {
                at = Some(off);
                break;
            }
            off += l.len() + 1;
        }
        let at = at?;
        let b = blocks.iter().find(|b| b.range.start <= at && at <= b.range.end)?;
        if b.kind == BlockKind::Quote {
            // the innermost callout holding the block id
            if let Some(c) = flatten(&parse_quote(text, b.range.clone())).into_iter().filter(|c| c.range.start <= at && at <= c.range.end).max_by_key(|c| c.header.start) {
                let start = text[..c.header.start].rfind('\n').map_or(0, |i| i + 1);
                return Some((cap(&text[start..c.range.end]), line_of(start)));
            }
        }
        return Some((cap(&text[b.range.clone()]), line_of(b.range.start)));
    }
    if sub.is_empty() {
        let start = blocks.iter().find(|b| b.kind != BlockKind::Frontmatter && b.kind != BlockKind::Blank).map_or(0, |b| b.range.start);
        return Some((cap(&text[start..]), line_of(start)));
    }
    // `#A#B` targets heading B
    let want = sub.rsplit('#').next().unwrap_or(sub).trim().to_lowercase();
    let (i, level) = blocks.iter().enumerate().find_map(|(i, b)| match b.kind {
        BlockKind::Heading { level } if text[b.content.clone()].trim().to_lowercase() == want => Some((i, level)),
        _ => None,
    })?;
    let end = blocks[i + 1..].iter().find(|b| matches!(b.kind, BlockKind::Heading { level: l } if l <= level)).map_or(text.len(), |b| b.range.start);
    let start = blocks[i].range.start;
    Some((cap(&text[start..end]), line_of(start)))
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn block_heading_and_note() {
        let t = "---\na: 1\n---\n# T\n\n## S\n> [!thm] x\n> body\n> > [!lemma]\n> > inner\n> > ^lem1\n\n## Next\nmore\n";
        let (s, line) = snippet(t, "^lem1").unwrap();
        assert_eq!((s.as_str(), line), ("> > [!lemma]\n> > inner\n> > ^lem1", 8));
        let (s, line) = snippet(t, "S").unwrap();
        assert!(s.starts_with("## S") && !s.contains("Next"), "{s}");
        assert_eq!(line, 5);
        assert!(snippet(t, "").unwrap().0.starts_with("# T"));
        assert!(snippet(t, "^nope").is_none());
    }
}
