//! The editing toolbar's text operations, as pure functions: (text, selection) → one
//! replacement and the selection afterwards. Byte offsets throughout.

use std::ops::Range;

use obsidilight_core::Selection;

/// Replace `range` with `text`, then select `sel` (offsets in the new text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub range: Range<usize>,
    pub text: String,
    pub sel: Selection,
}

fn line_start(t: &str, at: usize) -> usize {
    t[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(t: &str, at: usize) -> usize {
    t[at..].find('\n').map_or(t.len(), |i| at + i)
}

/// The whole lines the selection touches (a selection ending at a line start excludes it).
fn lines_range(t: &str, sel: &Range<usize>) -> Range<usize> {
    let end = if sel.end > sel.start && t[..sel.end].ends_with('\n') { sel.end - 1 } else { sel.end };
    line_start(t, sel.start)..line_end(t, end)
}

/// Rewrites each selected line; the new selection covers the rewritten lines.
fn map_lines(t: &str, sel: Range<usize>, f: impl Fn(usize, &str) -> String) -> Change {
    let r = lines_range(t, &sel);
    let out: Vec<String> = t[r.clone()].split('\n').enumerate().map(|(i, l)| f(i, l)).collect();
    let text = out.join("\n");
    let s = if sel.is_empty() {
        // keep the caret at the end of its (only) line
        Selection::caret(r.start + text.len())
    } else {
        Selection::new(r.start, r.start + text.len())
    };
    Change { range: r, text, sel: s }
}

/// Wraps the selection in `open`…`close`, or unwraps it if it already is (inside or just
/// outside the selection). With no selection: inserts the pair, caret in between (or removes
/// an empty pair around the caret).
pub fn toggle_wrap(t: &str, sel: Range<usize>, open: &str, close: &str) -> Change {
    let s = &t[sel.clone()];
    if !sel.is_empty() && s.len() >= open.len() + close.len() && s.starts_with(open) && s.ends_with(close) {
        let inner = &s[open.len()..s.len() - close.len()];
        return Change { range: sel.clone(), text: inner.to_string(), sel: Selection::new(sel.start, sel.start + inner.len()) };
    }
    let before = &t[..sel.start];
    let after = &t[sel.end..];
    if before.ends_with(open) && after.starts_with(close) {
        let r = sel.start - open.len()..sel.end + close.len();
        let start = sel.start - open.len();
        return Change { range: r, text: s.to_string(), sel: Selection::new(start, start + s.len()) };
    }
    let text = format!("{open}{s}{close}");
    let start = sel.start + open.len();
    Change { range: sel, text, sel: Selection::new(start, start + s.len()) }
}

fn heading_level(l: &str) -> (usize, &str) {
    let n = l.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&n) && l[n..].starts_with(' ') {
        (n, &l[n + 1..])
    } else {
        (0, l)
    }
}

/// Makes the lines headings of `level`; lines already at that level become plain.
pub fn heading(t: &str, sel: Range<usize>, level: usize) -> Change {
    map_lines(t, sel, |_, l| {
        let (n, rest) = heading_level(l);
        if n == level { rest.to_string() } else { format!("{} {rest}", "#".repeat(level)) }
    })
}

/// Leading whitespace, then a list marker (`- `, `* `, `1. `, `- [ ] `…) if any.
fn list_parts(l: &str) -> (&str, &str, &str) {
    let indent = &l[..l.len() - l.trim_start().len()];
    let rest = &l[indent.len()..];
    for task in ["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] "] {
        if let Some(r) = rest.strip_prefix(task) {
            return (indent, &rest[..task.len()], r);
        }
    }
    for b in ["- ", "* ", "+ "] {
        if let Some(r) = rest.strip_prefix(b) {
            return (indent, &rest[..2], r);
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") ")) {
        return (indent, &rest[..digits + 2], &rest[digits + 2..]);
    }
    (indent, "", rest)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum List {
    Bullet,
    Numbered,
}

/// Turns the lines into a list of that kind, or back into plain lines if they all already are.
pub fn toggle_list(t: &str, sel: Range<usize>, kind: List) -> Change {
    let r = lines_range(t, &sel);
    let is_kind = |m: &str| match kind {
        List::Bullet => m == "- " || m == "* " || m == "+ ",
        List::Numbered => m.ends_with(". ") || m.ends_with(") ") && m.starts_with(|c: char| c.is_ascii_digit()),
    };
    let all = t[r.clone()].split('\n').filter(|l| !l.trim().is_empty()).all(|l| is_kind(list_parts(l).1));
    map_lines(t, sel, |i, l| {
        if l.trim().is_empty() {
            return l.to_string();
        }
        let (indent, _, rest) = list_parts(l);
        if all {
            return format!("{indent}{rest}");
        }
        match kind {
            List::Bullet => format!("{indent}- {rest}"),
            List::Numbered => format!("{indent}{}. {rest}", i + 1),
        }
    })
}

/// Obsidian's "Toggle checklist status": a plain line becomes a task; a task is ticked or unticked.
pub fn toggle_checklist(t: &str, sel: Range<usize>) -> Change {
    map_lines(t, sel, |_, l| {
        let (indent, m, rest) = list_parts(l);
        match m {
            "- [ ] " | "* [ ] " => format!("{indent}- [x] {rest}"),
            "- [x] " | "- [X] " | "* [x] " => format!("{indent}- [ ] {rest}"),
            _ => format!("{indent}- [ ] {rest}"),
        }
    })
}

/// Plain → bullet → task → plain.
pub fn cycle_list(t: &str, sel: Range<usize>) -> Change {
    map_lines(t, sel, |_, l| {
        let (indent, m, rest) = list_parts(l);
        if m.contains('[') {
            format!("{indent}{rest}")
        } else if m.is_empty() {
            format!("{indent}- {rest}")
        } else {
            format!("{indent}- [ ] {rest}")
        }
    })
}

pub fn indent(t: &str, sel: Range<usize>) -> Change {
    map_lines(t, sel, |_, l| format!("\t{l}"))
}

pub fn outdent(t: &str, sel: Range<usize>) -> Change {
    map_lines(t, sel, |_, l| {
        if let Some(r) = l.strip_prefix('\t') {
            r.to_string()
        } else {
            let n = l.chars().take(4).take_while(|c| *c == ' ').count();
            l[n..].to_string()
        }
    })
}

pub fn toggle_quote(t: &str, sel: Range<usize>) -> Change {
    let r = lines_range(t, &sel);
    let all = t[r].split('\n').all(|l| l.starts_with('>'));
    map_lines(t, sel, |_, l| if all { l.strip_prefix("> ").or_else(|| l.strip_prefix('>')).unwrap_or(l).to_string() } else { format!("> {l}") })
}

/// Wraps the selected lines (or inserts at the caret's line) in a block: ```` ``` ````, `$$`, or a
/// callout; the caret goes where you type next.
pub fn block(t: &str, sel: Range<usize>, open: &str, close: &str) -> Change {
    let r = lines_range(t, &sel);
    let body = &t[r.clone()];
    let text = if close.is_empty() {
        // callout: every line quoted
        let lines: Vec<String> = body.split('\n').map(|l| format!("> {l}")).collect();
        format!("{open}\n{}", lines.join("\n"))
    } else {
        format!("{open}\n{body}\n{close}")
    };
    let caret = if body.is_empty() && !close.is_empty() { r.start + open.len() + 1 } else { r.start + open.len() };
    Change { range: r, text, sel: Selection::caret(caret) }
}

/// Inserts `text` at the end of the caret's line, on its own line(s).
pub fn insert_after_line(t: &str, sel: Range<usize>, text: &str) -> Change {
    let e = line_end(t, sel.end);
    let ins = format!("\n{text}");
    Change { range: e..e, text: ins.clone(), sel: Selection::caret(e + ins.len()) }
}

/// `[[sel]]`, `![[sel]]`, `[sel]()`: the caret lands where the target goes.
pub fn link(t: &str, sel: Range<usize>, kind: &str) -> Change {
    let s = &t[sel.clone()];
    let (text, caret) = match kind {
        "wikilink" => (format!("[[{s}]]"), sel.start + 2 + s.len()),
        "embed" => (format!("![[{s}]]"), sel.start + 3 + s.len()),
        _ => (format!("[{s}]()"), sel.start + s.len() + 3),
    };
    Change { range: sel, text, sel: Selection::caret(caret) }
}

/// Moves the selected lines one line up (`-1`) or down (`1`).
pub fn swap_lines(t: &str, sel: Range<usize>, dir: i32) -> Option<Change> {
    let r = lines_range(t, &sel);
    if dir < 0 {
        if r.start == 0 {
            return None;
        }
        let prev = line_start(t, r.start - 1)..r.start - 1;
        let text = format!("{}\n{}", &t[r.clone()], &t[prev.clone()]);
        let shift = r.start - prev.start;
        Some(Change { range: prev.start..r.end, text, sel: Selection::new(sel.start - shift, sel.end - shift) })
    } else {
        if r.end >= t.len() {
            return None;
        }
        let next = r.end + 1..line_end(t, r.end + 1);
        let text = format!("{}\n{}", &t[next.clone()], &t[r.clone()]);
        let shift = next.end - next.start + 1;
        Some(Change { range: r.start..next.end, text, sel: Selection::new(sel.start + shift, sel.end + shift) })
    }
}

/// Removes Markdown/HTML formatting from the selection (or the caret's line).
pub fn clear_formatting(t: &str, sel: Range<usize>) -> Change {
    let r = if sel.is_empty() { lines_range(t, &sel) } else { sel.clone() };
    let mut s = t[r.clone()].to_string();
    for m in ["**", "~~", "==", "`"] {
        s = s.replace(m, "");
    }
    let tags = regex::Regex::new(r"</?(u|sup|sub|font|span|center|p|mark)(\s[^>]*)?>").expect("regex");
    s = tags.replace_all(&s, "").into_owned();
    let em = regex::Regex::new(r"\*([^*\s][^*\n]*?)\*").expect("regex");
    s = em.replace_all(&s, "$1").into_owned();
    let len = s.len();
    Change { range: r.clone(), text: s, sel: Selection::new(r.start, r.start + len) }
}

/// Text colour or highlight colour, written as the Editing Toolbar plugin writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorKind {
    /// `<font color="#d83931">…</font>`
    Font,
    /// `<span style="background:#cdf4698c">…</span>`
    Background,
}

impl ColorKind {
    pub fn open(self, c: &str) -> String {
        match self {
            Self::Font => format!("<font color=\"{c}\">"),
            Self::Background => format!("<span style=\"background:{c}\">"),
        }
    }
    pub fn close(self) -> &'static str {
        match self {
            Self::Font => "</font>",
            Self::Background => "</span>",
        }
    }
    /// The opening tag, colour captured; `end` anchors it at the end of the text.
    fn open_re(self, end: bool) -> regex::Regex {
        let p = match self {
            Self::Font => r#"<font color="([^"]*)">"#,
            Self::Background => r#"<span style="background(?:-color)?:\s*([^";]*);?\s*">"#,
        };
        regex::Regex::new(&if end { format!("{p}$") } else { format!("^{p}") }).expect("regex")
    }
}

/// Colours the selection. `Some(c)`: wraps it in `kind`'s tag with colour `c`, or changes the
/// colour when it is already wrapped (the tags inside the selection or just around it); with
/// `toggle`, a selection already in `c` loses its colour instead (the button's second click).
/// `None` removes the colour. With no selection the tags go in around the caret.
pub fn set_color(t: &str, sel: Range<usize>, kind: ColorKind, color: Option<&str>, toggle: bool) -> Option<Change> {
    let close = kind.close();
    let s = &t[sel.clone()];
    // (outer range, inner range, current colour)
    let wrapped = if let Some(m) = kind.open_re(false).captures(s).filter(|_| s.ends_with(close)) {
        let open_len = m[0].len();
        (open_len + close.len() <= s.len()).then(|| (sel.clone(), sel.start + open_len..sel.end - close.len(), m[1].to_string()))
    } else {
        None
    };
    let wrapped = wrapped.or_else(|| {
        let m = kind.open_re(true).captures(&t[..sel.start])?;
        t[sel.end..].starts_with(close).then(|| (sel.start - m[0].len()..sel.end + close.len(), sel.clone(), m[1].to_string()))
    });
    match (wrapped, color) {
        (Some((outer, inner, cur)), c) => {
            let body = &t[inner.clone()];
            match c {
                Some(c) if !(toggle && cur.trim().eq_ignore_ascii_case(c)) => {
                    let open = kind.open(c);
                    let start = outer.start + open.len();
                    Some(Change { range: outer, text: format!("{open}{body}{close}"), sel: Selection::new(start, start + body.len()) })
                }
                _ => Some(Change { range: outer.clone(), text: body.to_string(), sel: Selection::new(outer.start, outer.start + body.len()) }),
            }
        }
        (None, None) => None,
        (None, Some(c)) => {
            let open = kind.open(c);
            let start = sel.start + open.len();
            Some(Change { range: sel, text: format!("{open}{s}{close}"), sel: Selection::new(start, start + s.len()) })
        }
    }
}

/// Line tools of the toolbar's "Text tools" menu.
pub fn lines_tool(t: &str, sel: Range<usize>, tool: &str) -> Option<Change> {
    let r = lines_range(t, &sel);
    let body = &t[r.clone()];
    let lines: Vec<&str> = body.split('\n').collect();
    let out: Vec<String> = match tool {
        "insert-blank-lines" => lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n\n").split('\n').map(String::from).collect(),
        "remove-blank-lines" => lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect(),
        "merge-lines" => vec![lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")],
        "split-lines" => lines.iter().flat_map(|l| l.split(". ").map(|s| s.to_string()).collect::<Vec<_>>()).collect(),
        "dedupe-lines" => {
            let mut seen = std::collections::HashSet::new();
            lines.iter().filter(|l| l.trim().is_empty() || seen.insert(l.to_string())).map(|l| l.to_string()).collect()
        }
        "number-lines" => lines.iter().enumerate().map(|(i, l)| format!("{}. {}", i + 1, list_parts(l).2)).collect(),
        "remove-whitespace-trim" => lines.iter().map(|l| l.trim_end().to_string()).collect(),
        "remove-whitespace-compress" => {
            let re = regex::Regex::new(r"[ \t]{2,}").expect("regex");
            lines.iter().map(|l| {
                let indent = &l[..l.len() - l.trim_start().len()];
                format!("{indent}{}", re.replace_all(l.trim_start(), " "))
            }).collect()
        }
        "remove-whitespace-all" => lines.iter().map(|l| l.split_whitespace().collect::<String>()).collect(),
        "list-to-table" => {
            let items: Vec<&str> = lines.iter().map(|l| list_parts(l).2).filter(|l| !l.trim().is_empty()).collect();
            let mut v = vec!["| Item |".to_string(), "| --- |".to_string()];
            v.extend(items.iter().map(|i| format!("| {} |", i.trim())));
            v
        }
        "table-to-list" => lines
            .iter()
            .filter(|l| {
                let c = l.trim().trim_matches('|').trim();
                !c.is_empty() && !c.chars().all(|ch| matches!(ch, '-' | ':' | '|' | ' '))
            })
            .map(|l| format!("- {}", l.trim().trim_matches('|').split('|').map(str::trim).collect::<Vec<_>>().join(", ")))
            .collect(),
        _ => return None,
    };
    let text = out.join("\n");
    let len = text.len();
    Some(Change { range: r.clone(), text, sel: Selection::new(r.start, r.start + len) })
}

/// The selection as plain text (formatting markers and link syntax removed).
pub fn plain_text(s: &str) -> String {
    let c = clear_formatting(s, 0..s.len()).text;
    let link = regex::Regex::new(r"!?\[\[([^\]|]*\|)?([^\]]*)\]\]").expect("regex");
    let md = regex::Regex::new(r"!?\[([^\]]*)\]\([^)]*\)").expect("regex");
    let c = link.replace_all(&c, "$2").into_owned();
    md.replace_all(&c, "$1").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(t: &str, c: &Change) -> String {
        format!("{}{}{}", &t[..c.range.start], c.text, &t[c.range.end..])
    }

    #[test]
    fn wrap_and_unwrap() {
        let t = "a word here";
        let c = toggle_wrap(t, 2..6, "**", "**");
        let t2 = apply(t, &c);
        assert_eq!(t2, "a **word** here");
        assert_eq!(c.sel.range(), 4..8, "the word stays selected");
        let c2 = toggle_wrap(&t2, c.sel.range(), "**", "**");
        assert_eq!(apply(&t2, &c2), t, "toggling again unwraps");
        let c3 = toggle_wrap(&t2, 2..10, "**", "**");
        assert_eq!(apply(&t2, &c3), t, "markers inside the selection");
        let c4 = toggle_wrap(t, 1..1, "==", "==");
        assert_eq!((apply(t, &c4), c4.sel.head), ("a==== word here".into(), 3));
        assert_eq!(apply("a <u>word</u> here", &toggle_wrap("a <u>word</u> here", 5..9, "<u>", "</u>")), t);
    }

    #[test]
    fn headings_lists_quotes() {
        let t = "uno\n## due\ntre";
        assert_eq!(apply(t, &heading(t, 0..0, 2)), "## uno\n## due\ntre");
        assert_eq!(apply(t, &heading(t, 5..5, 2)), "uno\ndue\ntre", "same level: off");
        assert_eq!(apply(t, &heading(t, 5..5, 3)), "uno\n### due\ntre");
        let l = "a\nb\n\nc";
        assert_eq!(apply(l, &toggle_list(l, 0..l.len(), List::Bullet)), "- a\n- b\n\n- c");
        let b = "- a\n- b";
        assert_eq!(apply(b, &toggle_list(b, 0..b.len(), List::Bullet)), "a\nb");
        assert_eq!(apply(b, &toggle_list(b, 0..b.len(), List::Numbered)), "1. a\n2. b");
        assert_eq!(apply(b, &toggle_checklist(b, 0..1)), "- [ ] a\n- b");
        assert_eq!(apply("- [ ] a", &toggle_checklist("- [ ] a", 0..0)), "- [x] a");
        assert_eq!(apply("x", &cycle_list("x", 0..0)), "- x");
        assert_eq!(apply("\t- x", &outdent("\t- x", 0..0)), "- x");
        assert_eq!(apply("a\nb", &toggle_quote("a\nb", 0..3)), "> a\n> b");
        assert_eq!(apply("> a\n> b", &toggle_quote("> a\n> b", 0..7)), "a\nb");
    }

    #[test]
    fn blocks_links_lines() {
        let c = block("x\ny", 0..3, "```", "```");
        assert_eq!(apply("x\ny", &c), "```\nx\ny\n```");
        assert_eq!(apply("x", &block("x", 0..0, "> [!note]", "")), "> [!note]\n> x");
        let c = link("see it", 4..6, "wikilink");
        assert_eq!((apply("see it", &c), c.sel.head), ("see [[it]]".into(), 8));
        let c = link("see it", 4..6, "md");
        assert_eq!((apply("see it", &c), c.sel.head), ("see [it]()".into(), 9));
        let t = "a\nb\nc";
        let c = swap_lines(t, 2..3, -1).unwrap();
        assert_eq!((apply(t, &c), c.sel.range()), ("b\na\nc".into(), 0..1));
        let c = swap_lines(t, 2..3, 1).unwrap();
        assert_eq!((apply(t, &c), c.sel.range()), ("a\nc\nb".into(), 4..5));
        assert!(swap_lines(t, 0..0, -1).is_none());
        assert_eq!(apply("**a** ==b== <u>c</u> *d*", &clear_formatting("**a** ==b== <u>c</u> *d*", 0..0)), "a b c d");
        let l = "- x\n- y\n- x";
        assert_eq!(apply(l, &lines_tool(l, 0..l.len(), "dedupe-lines").unwrap()), "- x\n- y");
        assert_eq!(apply(l, &lines_tool(l, 0..l.len(), "list-to-table").unwrap()), "| Item |\n| --- |\n| x |\n| y |\n| x |");
        assert_eq!(plain_text("**a** [[N|n]] [t](u)"), "a n t");
    }

    #[test]
    fn colours_wrap_change_and_remove() {
        use ColorKind::*;
        let t = "a word here";
        let set = |t: &str, sel: Range<usize>, k, c: Option<&str>, toggle| set_color(t, sel, k, c, toggle).map(|c| (apply(t, &c), c.sel));
        // wrap, the selection stays on the word
        let (t1, s1) = set(t, 2..6, Font, Some("#d83931"), false).unwrap();
        assert_eq!(t1, "a <font color=\"#d83931\">word</font> here");
        assert_eq!(&t1[s1.range()], "word");
        // another colour replaces it (selection inside, or covering the tags)
        let (t2, s2) = set(&t1, s1.range(), Font, Some("#245BDB"), false).unwrap();
        assert_eq!(t2, "a <font color=\"#245BDB\">word</font> here");
        let all = 2..t2.len() - 5;
        assert_eq!(set(&t2, all, Font, Some("#ff0000"), false).unwrap().0, "a <font color=\"#ff0000\">word</font> here");
        // the button again with the same colour removes it; picking it again keeps it
        assert_eq!(set(&t2, s2.range(), Font, Some("#245bdb"), true).unwrap().0, t);
        assert_eq!(set(&t2, s2.range(), Font, Some("#245BDB"), false).unwrap().0, t2);
        // remove
        let (t3, s3) = set(&t2, s2.range(), Font, None, false).unwrap();
        assert_eq!((t3.as_str(), &t3[s3.range()]), (t, "word"));
        assert_eq!(set(t, 2..6, Font, None, false), None, "nothing to remove");
        // background, also as Obsidian's plugin and others write it
        let (b1, bs) = set(t, 2..6, Background, Some("#CDF4698C"), false).unwrap();
        assert_eq!(b1, "a <span style=\"background:#CDF4698C\">word</span> here");
        assert_eq!(set(&b1, bs.range(), Background, Some("#A0CCF68C"), false).unwrap().0, "a <span style=\"background:#A0CCF68C\">word</span> here");
        let other = "a <span style=\"background-color: #ff0;\">word</span> here";
        let w = other.find("word").unwrap();
        assert_eq!(set(other, w..w + 4, Background, None, false).unwrap().0, t);
        // a font tag isn't a background and vice versa: nesting both works
        let both = set(&t1, s1.range(), Background, Some("#ff0"), false).unwrap().0;
        assert_eq!(both, "a <font color=\"#d83931\"><span style=\"background:#ff0\">word</span></font> here");
        // no selection: an empty pair, caret inside
        let (e, es) = set("ab", 1..1, Font, Some("#000"), false).unwrap();
        assert_eq!((e.as_str(), es.range()), ("a<font color=\"#000\"></font>b", 20..20));
    }
}
