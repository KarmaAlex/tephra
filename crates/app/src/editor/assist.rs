//! Typing assistance (core, like Obsidian): auto-paired brackets and quotes, and link
//! suggestions after `[[`. Pure functions over text; the editor applies their results.

use tephra_core::index::{is_note, VaultIndex};

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

// ---- Enter and fences -----------------------------------------------------------------------

/// What Enter does on a line: list items and blockquotes carry their markers to the new line.
#[derive(Debug, PartialEq, Eq)]
pub enum Enter {
    /// Replace the whole line with this text: Enter on an empty item or quote line ends it.
    ClearLine(String),
    /// Insert a line break followed by this prefix.
    Continue(String),
    /// A plain line break.
    Plain,
}

/// Enter on `line` with the caret at byte `col` of it (`has_selection`: replacing a selection).
pub fn on_enter(line: &str, col: usize, has_selection: bool) -> Enter {
    let ws = |s: &str| s.len() - s.trim_start_matches([' ', '\t']).len();
    let indent = ws(line);
    // quote markers: `>` each with one optional space (`> `, `>> `, `> > `)
    let (mut q, mut outer) = (indent, indent);
    while line[q..].starts_with('>') {
        outer = q;
        q += 1;
        if line[q..].starts_with([' ', '\t']) {
            q += 1;
        }
    }
    let quoted = q > indent;
    let lead = q + ws(&line[q..]); // indent, quote markers and the item's own indent
    let rest = &line[lead..];
    // (marker on this line, marker for the next line)
    let markers: Option<(String, String)> = ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "]
        .iter()
        .find(|m| rest.starts_with(**m))
        .map(|m| (m.to_string(), m.replace("[x]", "[ ]").replace("[X]", "[ ]")))
        .or_else(|| {
            let d: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let n: u32 = d.parse().ok()?;
            let sep = rest[d.len()..].chars().next().filter(|c| *c == '.' || *c == ')')?;
            rest[d.len() + 1..].starts_with(' ').then(|| (format!("{n}{sep} "), format!("{}{sep} ", n + 1)))
        });
    let empty = !has_selection && rest.trim().is_empty();
    match markers {
        // caret inside the markers: just push the line down
        Some((cur, _)) if col < lead + cur.len() => Enter::Plain,
        // empty item: end the list (staying in the quote it is in)
        Some((cur, _)) if !has_selection && rest.trim_end() == cur.trim_end() => {
            Enter::ClearLine(if quoted { line[..q].to_string() } else { String::new() })
        }
        Some((_, next)) => Enter::Continue(format!("{}{next}", &line[..lead])),
        None if !quoted || col < q => Enter::Plain,
        // empty quote line: leave one level of quoting (all of it when there is one)
        None if empty => Enter::ClearLine(if outer > indent { line[..outer].to_string() } else { String::new() }),
        None => {
            let mut p = line[..lead].to_string();
            if !p.ends_with([' ', '\t']) {
                p.push(' ');
            }
            Enter::Continue(p)
        }
    }
}

/// After typing the third backtick of a fence opener (```` ``` ```` alone on its line, caret at
/// its end), the closing fence to insert at the caret when the next line is empty or missing.
/// Not when the backticks close a fence opened above.
pub fn fence_closer(text: &str, caret: usize, nl: &str) -> Option<String> {
    let ls = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    let le = text[caret..].find('\n').map_or(text.len(), |i| caret + i);
    if !text[caret..le].trim_end_matches('\r').is_empty() {
        return None;
    }
    let line = &text[ls..caret];
    let body = line.trim_start_matches(' ');
    let indent = &line[..line.len() - body.len()];
    if body != "```" || indent.len() > 3 {
        return None;
    }
    if le < text.len() {
        let next = &text[le + 1..];
        if !next[..next.find('\n').unwrap_or(next.len())].trim().is_empty() {
            return None;
        }
    }
    let opens = tephra_core::markdown::parse_blocks(text)
        .iter()
        .any(|b| b.range.start == ls && matches!(b.kind, tephra_core::markdown::BlockKind::Code { .. }));
    opens.then(|| format!("{nl}{indent}```"))
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
    fn enter_continues_lists_and_quotes() {
        use Enter::*;
        let end = |l: &str| on_enter(l, l.len(), false);
        assert_eq!(end("- a"), Continue("- ".into()));
        assert_eq!(end("  - [x] a"), Continue("  - [ ] ".into()));
        assert_eq!(end("9) a"), Continue("10) ".into()));
        assert_eq!(end("- "), ClearLine(String::new()), "empty item ends the list");
        assert_eq!(end("plain"), Plain);
        assert_eq!(end("> a"), Continue("> ".into()));
        assert_eq!(end(">a"), Continue("> ".into()));
        assert_eq!(end("> > a"), Continue("> > ".into()));
        assert_eq!(end("> [!note] Title"), Continue("> ".into()), "callouts too");
        assert_eq!(end("> "), ClearLine(String::new()), "empty quote line ends the quote");
        assert_eq!(end(">> "), ClearLine(">".into()), "…one level at a time");
        assert_eq!(end("> - a"), Continue("> - ".into()), "lists inside quotes");
        assert_eq!(end("> - "), ClearLine("> ".into()), "empty item in a quote stays quoted");
        assert_eq!(on_enter("- a", 0, false), Plain, "caret before the marker");
        assert_eq!(on_enter("> a", 0, false), Plain);
        assert_eq!(on_enter("> ab", 3, false), Continue("> ".into()), "mid-line splits into the quote");
        assert_eq!(on_enter("> ", 2, true), Continue("> ".into()), "replacing a selection never clears");
    }

    #[test]
    fn fence_autoclose() {
        let at_end = |t: &str| fence_closer(t, t.find("```").unwrap() + 3, "\n");
        assert_eq!(at_end("```"), Some("\n```".into()));
        assert_eq!(at_end("a\n\n```\n\nb"), Some("\n```".into()));
        assert_eq!(at_end("  ```"), Some("\n  ```".into()), "keeps the indent");
        assert_eq!(at_end("```\nnext"), None, "next line not empty");
        assert_eq!(fence_closer("```js", 5, "\n"), None, "only right after the backticks");
        assert_eq!(fence_closer("```\ncode\n```", 12, "\n"), None, "closing an open fence");
        assert_eq!(fence_closer("x ```", 5, "\n"), None);
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
        std::fs::create_dir_all(root.join("Courses")).unwrap();
        std::fs::create_dir_all(root.join("Images")).unwrap();
        std::fs::write(root.join("Courses/Graphs.md"), "# Graphs\n## Basics\ntext ^blk1\n").unwrap();
        std::fs::write(root.join("Courses/Trees.md"), "x").unwrap();
        std::fs::write(root.join("Note.md"), "x").unwrap();
        std::fs::write(root.join("Images/graph.png"), "x").unwrap();
        let idx = VaultIndex::build(&root);
        let labels = |q: &str| suggestions(&idx, "Note.md", q, 10).into_iter().map(|s| s.label).collect::<Vec<_>>();
        assert_eq!(labels("grap"), vec!["Graphs", "graph.png"], "notes before attachments");
        assert_eq!(labels("tre"), vec!["Trees"], "start of a word");
        assert_eq!(labels("cou gra")[0], "Graphs", "fuzzy over the path");
        assert_eq!(suggestions(&idx, "Note.md", "Graphs", 1)[0].insert, "Graphs", "shortest link");
        assert_eq!(labels("Graphs#bas"), vec!["Basics"]);
        assert_eq!(suggestions(&idx, "Note.md", "Graphs#^", 5)[0].insert, "Graphs#^blk1");
        let _ = std::fs::remove_dir_all(&root);
    }
}
