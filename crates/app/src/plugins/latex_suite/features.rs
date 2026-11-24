//! Tabout, reverse tabout, auto-fraction, matrix shortcuts, auto-enlarge brackets and
//! auto-delete `$` (Latex Suite `features/*`, MIT).

use std::ops::Range;

use obsidilight_core::{EditKind, Selection};
use regex::Regex;

use super::context::{Bounds, Ctx, EnvName};
use super::tex::{find_matching_bracket, keep_indent_and_callout, open_bracket_for, tokenize, Token};
use super::Settings;
use crate::editor::EditorApi;

const LEFT_COMMANDS: &[&str] = &["\\left", "\\bigl", "\\Bigl", "\\biggl", "\\Biggl"];
const RIGHT_COMMANDS: &[&str] = &["\\right", "\\bigr", "\\Bigr", "\\biggr", "\\Biggr"];
const DELIMITERS: &[&str] = &[
    "(", ")", "[", "]", "\\lbrack", "\\rbrack", "\\{", "\\}", "\\lbrace", "\\rbrace", "<", ">", "\\langle", "\\rangle", "\\lt", "\\gt", "|", "\\vert", "\\lvert", "\\rvert", "\\|", "\\Vert", "\\lVert", "\\rVert", "\\lfloor",
    "\\rfloor", "\\lceil", "\\rceil", "\\ulcorner", "\\urcorner", "/", "\\\\", "\\backslash", "\\uparrow", "\\downarrow", "\\Uparrow", "\\Downarrow", ".",
];
/// Opening -> closing delimiter.
const DELIMITERS_MAP: &[(&str, &str)] = &[
    ("(", ")"),
    ("[", "]"),
    ("{", "}"),
    ("\\lbrack", "\\rbrack"),
    ("\\lbrace", "\\rbrace"),
    ("\\langle", "\\rangle"),
    ("\\lvert", "\\rvert"),
    ("\\lVert", "\\rVert"),
    ("\\lfloor", "\\rfloor"),
    ("\\lceil", "\\rceil"),
    ("\\ulcorner", "\\urcorner"),
    ("<", ">"),
];

fn line_start(text: &str, p: usize) -> usize {
    text[..p].rfind('\n').map_or(0, |i| i + 1)
}
fn line_end(text: &str, p: usize) -> usize {
    text[p..].find('\n').map_or(text.len(), |i| p + i)
}
fn multiline(text: &str, b: &Bounds) -> bool {
    text[b.outer_start..b.outer_end.min(text.len())].contains('\n')
}

/// Applies edits (ranges in the current text, any order, non-overlapping) and puts the caret at
/// `caret` (an offset in the text *before* the edits, mapped through them).
fn apply_edits(e: &mut EditorApi, mut edits: Vec<(Range<usize>, String)>, caret: usize) {
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut c = caret as isize;
    for (r, t) in &edits {
        if r.end <= caret {
            c += t.len() as isize - (r.end - r.start) as isize;
        }
    }
    for (r, t) in edits {
        e.replace(r, &t, EditKind::Other);
    }
    e.set_caret(c.max(0) as usize);
}

// ---- tabout ---------------------------------------------------------------------------

fn is_closing_delimiter(tokens: &[Token], i: usize, closing: &[String]) -> bool {
    let cur = &tokens[i];
    if i > 0 {
        let prev = tokens[i - 1].text.as_str();
        if RIGHT_COMMANDS.contains(&prev) && DELIMITERS.contains(&cur.text.as_str()) {
            return true;
        }
        if LEFT_COMMANDS.contains(&prev) && DELIMITERS.contains(&cur.text.as_str()) {
            return false;
        }
    }
    closing.iter().any(|c| *c == cur.text)
}

pub fn tabout(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    let sel = e.selection();
    if !sel.is_empty() {
        return false;
    }
    let Some(b) = ctx.bounds else { return false };
    let pos = sel.head;
    if b.outer_end <= pos {
        return false;
    }
    let tex = &text[b.inner_start..b.inner_end];
    let rel = pos - b.inner_start;
    let tokens = tokenize(tex);
    let start = tokens.iter().position(|t| t.end > rel).unwrap_or(tokens.len());
    for i in start..tokens.len() {
        let t = &tokens[i];
        // an empty `{}` after the caret (the end of `${}$0{}$`) encloses nothing to leave
        if t.text == "}" && i > 0 && tokens[i - 1].text == "{" && tokens[i - 1].start >= rel {
            continue;
        }
        let unmatched_right = RIGHT_COMMANDS.contains(&t.text.as_str()) && tokens.get(i + 1).is_none_or(|n| !DELIMITERS.contains(&n.text.as_str()));
        if is_closing_delimiter(&tokens, i, &s.tabout_closing) || unmatched_right {
            e.set_caret(b.inner_start + t.end);
            return true;
        }
    }
    let at_end = text[pos..b.inner_end].replace("{}", "").trim().is_empty();
    if !at_end && s.tabout_exit_on_eol {
        return false;
    }
    if !multiline(text, &b) {
        e.set_caret(b.outer_end);
        return true;
    }
    // multi-line display math: go to the line after the closing $$ (creating it if needed),
    // trimming trailing whitespace on the caret's line
    let end_line_end = line_end(text, line_start(text, b.outer_end));
    let start_ls = line_start(text, b.outer_start);
    let indent: String = text[start_ls..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let (cl_s, cl_e) = (line_start(text, pos), line_end(text, pos));
    let trim = if cl_e < end_line_end { cl_e - (cl_s + text[cl_s..cl_e].trim_end().len()) } else { 0 };
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let caret = if end_line_end >= text.len() {
        edits.push((end_line_end..end_line_end, format!("\n{indent}")));
        end_line_end + 1 + indent.len()
    } else {
        let after_s = end_line_end + 1;
        let after_e = line_end(text, after_s);
        if text[after_s..after_e].trim().is_empty() {
            edits.push((after_s..after_e, indent.clone()));
            after_s + indent.len()
        } else {
            after_s
        }
    };
    if trim > 0 {
        edits.push((cl_e - trim..cl_e, String::new()));
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (r, t) in edits {
        e.replace(r, &t, EditKind::Other);
    }
    e.set_caret(caret - trim);
    true
}

pub fn reverse_tabout(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    let sel = e.selection();
    if !sel.is_empty() {
        return false;
    }
    let Some(b) = ctx.bounds else { return false };
    let pos = sel.head;
    if pos < b.inner_start || pos > b.inner_end {
        return false;
    }
    let tokens = tokenize(&text[b.inner_start..pos]);
    for i in (0..tokens.len()).rev() {
        let t = &tokens[i];
        if LEFT_COMMANDS.contains(&t.text.as_str()) {
            e.set_caret(b.inner_start + t.start);
            return true;
        }
        if DELIMITERS_MAP.iter().any(|(o, _)| *o == t.text) {
            let at = if i > 0 && LEFT_COMMANDS.contains(&tokens[i - 1].text.as_str()) { tokens[i - 1].start } else { t.start };
            e.set_caret(b.inner_start + at);
            return true;
        }
    }
    if !s.tabout_exit_on_eol || text[b.inner_start..pos].trim().is_empty() {
        e.set_caret(b.outer_start);
    }
    false
}

/// `)`, `]`, `}` typed right before the same character, or `)`/`]` before the ` \right)` /
/// ` \right]` that auto-enlarge made of it: step over it.
pub fn tabout_by_close_bracket(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str, key: &str) -> bool {
    let sel = e.selection();
    if !sel.is_empty() || !matches!(key, ")" | "]" | "}") {
        return false;
    }
    let rest = &text[sel.head..];
    let enlarged = key != "}" && rest.trim_start_matches(' ').strip_prefix("\\right").is_some_and(|r| r.starts_with(key));
    if !rest.starts_with(key) && !enlarged {
        return false;
    }
    tabout(e, s, ctx, text)
}

// ---- auto-fraction ----------------------------------------------------------------------

fn greek_space() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new("(alpha|beta|gamma|Gamma|delta|Delta|epsilon|varepsilon|zeta|eta|theta|Theta|iota|kappa|lambda|Lambda|mu|nu|omicron|xi|Xi|pi|Pi|rho|sigma|Sigma|tau|upsilon|Upsilon|varphi|phi|Phi|chi|psi|Psi|omega|Omega) ([^ ])").expect("regex")
    })
}

/// A numerator wrapped in one pair of parentheses, `( … )` or `\left( … \right)`, loses them.
fn strip_parens(num: &str) -> &str {
    if num.starts_with('(') && num.ends_with(')') && find_matching_bracket(num, 0, "(", ")", false) == Some(num.len() - 1) {
        return &num[1..num.len() - 1];
    }
    if let Some(inner) = num.strip_prefix("\\left(").and_then(|r| r.strip_suffix("\\right)")) {
        // the `(` must match the final `)`, not close before it: `\left(a\right)+\left(b\right)`
        let open = "\\left".len();
        if find_matching_bracket(num, open, "(", ")", false) == Some(num.len() - 1) {
            return inner.trim();
        }
    }
    num
}

pub fn autofraction(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    if !s.autofraction || !ctx.mode.strictly_in_math() {
        return false;
    }
    if s.autofraction_excluded.iter().any(|(o, c)| ctx.is_within(text, o, c)) {
        return false;
    }
    let Some(b) = ctx.bounds else { return false };
    let sel = e.selection().range();
    let (from, to) = (sel.start, sel.end);
    let mut start = b.inner_start;
    if from != to {
        start = from;
    } else {
        // spaces after Greek letters do not break the numerator
        let cur = greek_space().replace_all(&text[b.inner_start..to], "$1#$2").into_owned();
        let breaking = format!(" $([{{\n{}", s.autofraction_breaking);
        let bytes = cur.as_bytes();
        let mut i = bytes.len() as isize - 1;
        while i >= 0 {
            let c = bytes[i as usize] as char;
            if let Some(open) = open_bracket_for(c) {
                match find_matching_bracket(&cur, i as usize, &open.to_string(), &c.to_string(), true) {
                    Some(j) => i = j as isize,
                    None => return false,
                }
            }
            if c.is_ascii() && breaking.contains(c) {
                start = i as usize + 1 + b.inner_start;
                break;
            }
            i -= 1;
        }
    }
    // empty groups in front (the `{}` of the `${}$0{}$` inline-math snippet) stay outside
    while text[start..to].starts_with("{}") {
        start += 2;
    }
    if start == to {
        return false;
    }
    let num = strip_parens(&text[start..to]);
    let template = if num.is_empty() { format!("{}{{$0}}{{$1}}$2", s.autofraction_symbol) } else { format!("{}{{{}}}{{$1}}$2", s.autofraction_symbol, num.replace('$', "\u{E001}")) };
    let mut ins = super::snippets::expand_tabstops(&template);
    ins.text = ins.text.replace('\u{E001}', "$");
    let key = (from == to).then_some("/");
    super::apply_insert(e, start, to, ins, key, text);
    enlarge_brackets(e, s);
    true
}

// ---- matrix shortcuts ---------------------------------------------------------------------

/// The matrix-like environment the caret is directly in: Some(env bounds) (None for macros).
fn matrix_env(s: &Settings, ctx: &Ctx) -> Option<Option<Bounds>> {
    if !s.matrix_shortcuts || !ctx.mode.strictly_in_math() || ctx.bounds.is_none() {
        return None;
    }
    match ctx.innermost()? {
        EnvName::Environment { name, bounds } if s.matrix_envs.contains(name) => Some(Some(*bounds)),
        EnvName::Command { name, .. } if s.matrix_macros.contains(name) => Some(None),
        _ => None,
    }
}

pub fn matrix_newline(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    let Some(env) = matrix_env(s, ctx) else { return false };
    let pos = ctx.pos;
    let ls = line_start(text, pos);
    let line = &text[ls..line_end(text, pos)];
    static CELLS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = CELLS.get_or_init(|| Regex::new(r"(\\begin\{.\]*\}|\\\\|^)((?:\s|&)+)").expect("regex"));
    let added = re.captures(line).and_then(|c| c.get(2)).map(|m| m.as_str().trim_start().to_string()).unwrap_or_default();
    let sel = e.selection().range();
    if env.is_some_and(|b| multiline(text, &b)) {
        let mut ts = vec![(0usize, 0..0)];
        let raw = format!(" \\\\\n{added}");
        ts[0].1 = raw.len()..raw.len();
        let t = keep_indent_and_callout(&raw, &mut ts, &text[ls..pos]);
        e.insert_snippet(sel, &t, &ts, EditKind::Other);
    } else {
        e.replace(sel, &format!(" \\\\  {added}"), EditKind::Other);
    }
    true
}

pub fn matrix_cell(e: &mut EditorApi, s: &Settings, ctx: &Ctx) -> bool {
    if matrix_env(s, ctx).is_none() || !e.selection().is_empty() {
        return false;
    }
    let r = e.selection().range();
    e.replace(r, " & ", EditKind::Other);
    true
}

pub fn matrix_exit(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    let Some(env) = matrix_env(s, ctx) else { return false };
    match env {
        Some(b) if multiline(text, &b) => {
            let next_s = line_end(text, ctx.pos) + 1;
            if next_s > text.len() {
                return true;
            }
            let next_e = line_end(text, next_s);
            let line = &text[next_s..next_e];
            let mut to = next_e;
            if let Some(c) = Regex::new(r"\\end\{([^}]*)\}").ok().and_then(|re| re.captures(line)) {
                if s.matrix_envs.iter().any(|n| n == &c[1]) {
                    to = next_s + c.get(0).map_or(0, |m| m.end());
                }
            }
            e.set_caret(to);
        }
        Some(b) => e.set_caret(b.outer_end),
        None => e.set_caret(ctx.bounds.map_or(ctx.pos, |b| b.outer_end)),
    }
    true
}

/// Tab inside a matrix: leave an enclosing bracket on this line before adding a cell.
pub fn matrix_priority_tabout(e: &mut EditorApi, s: &Settings, ctx: &Ctx, text: &str) -> bool {
    if !s.tabout || matrix_env(s, ctx).is_none() {
        return false;
    }
    let pos = ctx.pos;
    let rest = &text[pos..line_end(text, pos)];
    let closing: Vec<&str> = DELIMITERS_MAP.iter().map(|(_, c)| *c).filter(|c| s.tabout_closing.iter().any(|x| x == c)).collect();
    let opening: Vec<&str> = DELIMITERS_MAP.iter().filter(|(_, c)| closing.contains(c)).map(|(o, _)| *o).collect();
    let mut depth = 0;
    for t in tokenize(rest) {
        if closing.contains(&t.text.as_str()) {
            if depth == 0 {
                e.set_caret(pos + t.end);
                return true;
            }
            depth -= 1;
        } else if opening.contains(&t.text.as_str()) {
            depth += 1;
        }
    }
    false
}

// ---- auto-enlarge brackets -----------------------------------------------------------------

const SIZE_CONTROLS: &[&str] = &["\\big", "\\Big", "\\bigg", "\\Bigg", "\\bigl", "\\Bigl", "\\biggl", "\\Biggl", "\\bigr", "\\Bigr", "\\biggr", "\\Biggr", "\\left", "\\right"];
const PAIRS: &[(&str, &str)] = &[("(", ")"), ("[", "]"), ("{", "}"), ("\\{", "\\}"), ("\\langle", "\\rangle"), ("\\lbrack", "\\rbrack"), ("\\lfloor", "\\rfloor"), ("\\lceil", "\\rceil"), ("\\lvert", "\\rvert"), ("\\lVert", "\\rVert")];

/// After a snippet inserted `\sum`, `\frac`…: `( … )` around it become `\left( … \right)`.
pub fn enlarge_brackets(e: &mut EditorApi, s: &Settings) -> bool {
    if !s.enlarge_brackets {
        return false;
    }
    let text = e.text();
    let pos = e.selection().head;
    let Some(m) = crate::editor::api::math_at(&text, pos) else { return false };
    let tex = &text[m.inner.clone()];
    let tokens = tokenize(tex);
    let mut stack: Vec<(usize, &str)> = Vec::new();
    let mut edits = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if let Some((open, _)) = PAIRS.iter().find(|(o, _)| *o == t.text) {
            stack.push((i, open));
            continue;
        }
        let Some((open, _)) = PAIRS.iter().find(|(_, c)| *c == t.text) else { continue };
        let Some(k) = stack.iter().rposition(|(_, o)| o == open) else { continue };
        let (oi, _) = stack[k];
        stack.truncate(k);
        if *open == "{" {
            continue;
        }
        if oi > 0 && SIZE_CONTROLS.contains(&tokens[oi - 1].text.as_str()) {
            continue;
        }
        let o = &tokens[oi];
        let content = &tex[o.end..t.start];
        if !s.enlarge_triggers.iter().any(|w| content.contains(w.as_str())) {
            continue;
        }
        let sp = if s.enlarge_space { " " } else { "" };
        let base = m.inner.start;
        edits.push((base + o.start..base + o.end, format!("\\left{}{sp}", o.text)));
        edits.push((base + t.start..base + t.end, format!("{sp}\\right{}", t.text)));
    }
    if edits.is_empty() {
        return false;
    }
    let sel = e.selection();
    let ts_before = e.has_tabstops();
    apply_edits(e, edits, pos);
    // keep a selected placeholder selected
    if !sel.is_empty() && ts_before {
        let head = e.selection().head;
        let len = sel.range().len();
        e.set_selection(Selection::new(head - len.min(head), head));
    }
    true
}

// ---- auto-delete $ -------------------------------------------------------------------------

/// Backspace inside an empty `$|$` (or `${}|{}$`) deletes the whole formula.
pub fn auto_delete_dollar(e: &mut EditorApi, s: &Settings, text: &str) -> bool {
    let sel = e.selection();
    if !s.auto_delete_dollar || !sel.is_empty() {
        return false;
    }
    let p = sel.head;
    let b = text.as_bytes();
    // the empty `${}|{}$` the inline-math snippet leaves
    if text[..p].ends_with("${}") && text[p..].starts_with("{}$") && !text[..p - 3].ends_with('$') && !text[p + 3..].starts_with('$') {
        e.replace(p - 3..p + 3, "", EditKind::Delete);
        return true;
    }
    if p == 0 || p >= b.len() || b[p - 1] != b'$' || b[p] != b'$' {
        return false;
    }
    if (p >= 2 && b[p - 2] == b'$') || b.get(p + 1) == Some(&b'$') {
        return false; // `$$|$$` and friends are not an empty inline formula
    }
    e.replace(p - 1..p + 1, "", EditKind::Delete);
    true
}
