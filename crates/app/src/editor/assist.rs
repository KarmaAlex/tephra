//! Typing assistance (core, like Obsidian): auto-paired brackets and quotes, and link
//! suggestions after `[[`. Pure functions over text; the editor applies their results.

use obsidilight_core::index::{is_note, VaultIndex};

// ---- auto-pairing -------------------------------------------------------------------------

/// The closing partner of a character that auto-pairs.
pub fn closer(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_quote(c: char) -> bool {
    c == '"' || c == '`'
}

/// What typing `ch` should do (CodeMirror's closeBrackets rules, as in Obsidian).
#[derive(Debug, PartialEq, Eq)]
pub enum Pair {
    /// Insert this text and put the caret `caret_back` bytes before its end.
    Insert { text: String, caret_back: usize },
    /// The next character is this closer: just move over it.
    StepOver,
    /// Wrap the selection in the pair (keeping it selected).
    Wrap { open: char, close: char },
    /// Normal typing.
    None,
}

pub fn on_type(ch: char, before: Option<char>, after: Option<char>, has_selection: bool) -> Pair {
    if has_selection {
        return match closer(ch) {
            Some(close) => Pair::Wrap { open: ch, close },
            None => Pair::None,
        };
    }
    // typing the closer that is already there
    if after == Some(ch) && (matches!(ch, ')' | ']' | '}') || is_quote(ch)) {
        return Pair::StepOver;
    }
    let Some(close) = closer(ch) else { return Pair::None };
    // pair only before whitespace, the end of the line, or a closing bracket/punctuation
    let after_ok = after.is_none_or(|c| c.is_whitespace() || ")]}:;,.".contains(c));
    // quotes: not right after a word or the same quote (apostrophes, ``` fences)
    let before_ok = !is_quote(ch) || before.is_none_or(|c| !(c.is_alphanumeric() || c == ch));
    if after_ok && before_ok {
        Pair::Insert { text: format!("{ch}{close}"), caret_back: close.len_utf8() }
    } else {
        Pair::None
    }
}

/// Backspace between an empty pair deletes both halves.
pub fn deletes_pair(before: Option<char>, after: Option<char>) -> bool {
    matches!((before, after), (Some(b), Some(a)) if closer(b) == Some(a))
}

// ---- link suggestions -----------------------------------------------------------------------

/// An open `[[…` before the caret on its line: (offset just after `[[`, the query, embed?).
pub fn link_query(line: &str, caret: usize) -> Option<(usize, String, bool)> {
    let before = &line[..caret];
    let open = before.rfind("[[")?;
    let query = &before[open + 2..];
    if query.contains("]]") || query.contains('\n') || query.contains('|') || query.len() > 200 {
        return None;
    }
    let embed = before[..open].ends_with('!');
    Some((open + 2, query.to_string(), embed))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// Shown text.
    pub label: String,
    /// Muted detail (folder, level…).
    pub detail: String,
    /// Replaces the query (the text after `[[` up to the caret).
    pub insert: String,
}

fn name_of(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".md").unwrap_or(file)
}

/// Lower score = better; None = no match. Prefix of the name, start of a word in it, inside it,
/// inside the path, then characters in order anywhere.
fn score(query: &str, name: &str, path: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(5);
    }
    let (q, n, p) = (query.to_lowercase(), name.to_lowercase(), path.to_lowercase());
    if n == q {
        return Some(0);
    }
    if n.starts_with(&q) {
        return Some(1);
    }
    if n.split([' ', '-', '_', '.']).any(|w| w.starts_with(&q)) {
        return Some(2);
    }
    if n.contains(&q) {
        return Some(3);
    }
    if p.contains(&q) {
        return Some(4);
    }
    let mut it = p.chars();
    q.chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|x| x == c)).then_some(6)
}

/// Suggestions for the query after `[[` in the note `from` (vault path).
pub fn suggestions(index: &VaultIndex, from: &str, query: &str, limit: usize) -> Vec<Suggestion> {
    if let Some((note, sub)) = query.split_once('#') {
        // headings / block ids of a note (empty = this note)
        let target = if note.is_empty() { Some(from.to_string()) } else { index.resolve(note, from) };
        let Some(meta) = target.as_deref().and_then(|t| index.note(t)) else { return Vec::new() };
        if let Some(b) = sub.strip_prefix('^') {
            return meta
                .block_ids
                .iter()
                .filter(|(id, _)| id.to_lowercase().contains(&b.to_lowercase()))
                .take(limit)
                .map(|(id, line)| Suggestion { label: format!("^{id}"), detail: format!("line {}", line + 1), insert: format!("{note}#^{id}") })
                .collect();
        }
        let mut hs: Vec<(u32, usize, Suggestion)> = meta
            .headings
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                score(sub, &h.text, &h.text).map(|s| (s, i, Suggestion { label: h.text.clone(), detail: format!("H{}", h.level), insert: format!("{note}#{}", h.text) }))
            })
            .collect();
        if !sub.is_empty() {
            hs.sort_by_key(|(s, i, _)| (*s, *i));
        }
        return hs.into_iter().take(limit).map(|(_, _, s)| s).collect();
    }
    let mut c: Vec<(u32, bool, usize, &String)> =
        index.files().filter_map(|f| score(query, name_of(f), f).map(|s| (s, !is_note(f), f.len(), f))).collect();
    // best match, notes before other files, shorter paths, then A→Z
    c.sort_by(|a, b| (a.0, a.1, a.2, a.3).cmp(&(b.0, b.1, b.2, b.3)));
    c.into_iter()
        .take(limit)
        .map(|(_, _, _, f)| {
            let folder = f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
            let label = if is_note(f) { name_of(f).to_string() } else { f.rsplit('/').next().unwrap_or(f).to_string() };
            Suggestion { label, detail: folder, insert: index.shortest_link(f, from) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_rules() {
        assert_eq!(on_type('(', Some('a'), None, false), Pair::Insert { text: "()".into(), caret_back: 1 });
        assert_eq!(on_type('(', None, Some('x'), false), Pair::None, "not before a word");
        assert_eq!(on_type('[', Some('['), Some(']'), false), Pair::Insert { text: "[]".into(), caret_back: 1 }, "[[ -> [[]]");
        assert_eq!(on_type(')', Some('a'), Some(')'), false), Pair::StepOver);
        assert_eq!(on_type('"', Some('l'), None, false), Pair::None, "no pair after a letter");
        assert_eq!(on_type('`', Some('`'), None, false), Pair::None, "``` fences");
        assert_eq!(on_type('\'', None, None, false), Pair::None, "apostrophes never pair");
        assert_eq!(on_type('(', None, None, true), Pair::Wrap { open: '(', close: ')' });
        assert!(deletes_pair(Some('('), Some(')')) && !deletes_pair(Some('('), Some(']')));
    }

    #[test]
    fn link_query_detection() {
        assert_eq!(link_query("see [[Gra", 9), Some((6, "Gra".into(), false)));
        assert_eq!(link_query("![[img", 6), Some((3, "img".into(), true)));
        assert_eq!(link_query("[[a]] b", 7), None);
        assert_eq!(link_query("[[a|b", 5), None);
    }

    #[test]
    fn note_heading_and_block_suggestions() {
        let root = std::env::temp_dir().join(format!("ol-suggest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Algoritmi")).unwrap();
        std::fs::create_dir_all(root.join("Images")).unwrap();
        std::fs::write(root.join("Algoritmi/Grafi.md"), "# Grafi\n## Cammini minimi\ntesto ^blk1\n").unwrap();
        std::fs::write(root.join("Algoritmi/Alberi di copertura.md"), "x").unwrap();
        std::fs::write(root.join("Note.md"), "x").unwrap();
        std::fs::write(root.join("Images/grafo.png"), "x").unwrap();
        let idx = VaultIndex::build(&root);
        let labels = |q: &str| suggestions(&idx, "Note.md", q, 10).into_iter().map(|s| s.label).collect::<Vec<_>>();
        assert_eq!(labels("graf"), vec!["Grafi", "grafo.png"], "notes before attachments");
        assert_eq!(labels("cop"), vec!["Alberi di copertura"], "start of a word");
        assert_eq!(labels("alg gr")[0], "Grafi", "fuzzy over the path");
        assert_eq!(suggestions(&idx, "Note.md", "Grafi", 1)[0].insert, "Grafi", "shortest link");
        assert_eq!(labels("Grafi#cam"), vec!["Cammini minimi"]);
        assert_eq!(suggestions(&idx, "Note.md", "Grafi#^", 5)[0].insert, "Grafi#^blk1");
        let _ = std::fs::remove_dir_all(&root);
    }
}
