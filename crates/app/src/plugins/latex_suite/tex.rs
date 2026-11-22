//! Small TeX helpers ported from Latex Suite (MIT): tokenizer, bracket matching, the
//! environment/command stack at a position, and a line's quote/indent prefix.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Latex Suite's tokenizer: control sequences (`\name` or `\x`), `%` comments, and single
/// characters; whitespace is skipped. Offsets are byte offsets into `s`.
pub fn tokenize(s: &str) -> Vec<Token> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = s[i..].chars().next().expect("char boundary");
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        match c {
            '%' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            '\\' => {
                i += 1;
                if i < b.len() && b[i].is_ascii_alphabetic() {
                    while i < b.len() && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                } else if i < b.len() {
                    i += s[i..].chars().next().map_or(1, char::len_utf8);
                }
            }
            _ => i += c.len_utf8(),
        }
        out.push(Token { start, end: i, text: s[start..i].to_string() });
    }
    out
}

/// Position of the bracket matching the one at `start` (`open`/`close` are the bracket
/// strings), searching forward or backward. Latex Suite's `findMatchingBracket`.
pub fn find_matching_bracket(s: &str, start: usize, open: &str, close: &str, backwards: bool) -> Option<usize> {
    let mut depth = 0i32;
    if backwards {
        let mut i = start as isize;
        while i >= 0 {
            let at = i as usize;
            if s.is_char_boundary(at) {
                if s[at..].starts_with(close) {
                    depth += 1;
                } else if s[at..].starts_with(open) {
                    depth -= 1;
                    if depth == 0 {
                        return Some(at);
                    }
                }
            }
            i -= 1;
        }
    } else {
        let mut i = start;
        while i < s.len() {
            if s.is_char_boundary(i) {
                if s[i..].starts_with(open) {
                    depth += 1;
                } else if s[i..].starts_with(close) {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
    }
    None
}

pub fn open_bracket_for(close: char) -> Option<char> {
    match close {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

pub fn close_bracket_for(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

/// An enclosing construct at a position inside math, innermost last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// `\begin{name}` … `\end{name}`; `start` = offset of `\begin`.
    Env { name: String, start: usize },
    /// The `arg`-th (0-based) braced argument of `\name`; `start` = offset of its `{`.
    Cmd { name: String, arg: usize, start: usize },
    /// A bare `{…}` group.
    Group { start: usize },
}

/// The stack of environments, command arguments and groups enclosing byte offset `upto` of
/// `tex` (the equation's inner text).
pub fn stack_at(tex: &str, upto: usize) -> Vec<Frame> {
    let b = tex.as_bytes();
    let upto = upto.min(b.len());
    let mut stack: Vec<Frame> = Vec::new();
    // the command whose next `{` is an argument, and how many arguments it already had
    let mut pending: Option<(String, usize)> = None;
    let mut i = 0;
    while i < upto {
        match b[i] {
            b'%' => {
                while i < upto && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'\\' => {
                let s = i + 1;
                let mut j = s;
                while j < b.len() && b[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j == s {
                    // escaped character (`\{`, `\\`, …)
                    pending = None;
                    i = (s + tex[s.min(tex.len())..].chars().next().map_or(1, char::len_utf8)).max(s + 1);
                    continue;
                }
                let name = &tex[s..j];
                if name == "begin" || name == "end" {
                    // `\begin{env}` / `\end{env}` closed before `upto`: an environment boundary
                    let mut k = j;
                    while k < b.len() && b[k] == b' ' {
                        k += 1;
                    }
                    if k < b.len() && b[k] == b'{' {
                        if let Some(close) = tex[k..].find('}').map(|c| k + c) {
                            if close < upto {
                                let env = tex[k + 1..close].to_string();
                                if name == "begin" {
                                    stack.push(Frame::Env { name: env, start: i });
                                } else if let Some(p) = stack.iter().rposition(|f| matches!(f, Frame::Env { name: n, .. } if *n == env)) {
                                    stack.truncate(p);
                                }
                                pending = None;
                                i = close + 1;
                                continue;
                            }
                        }
                    }
                }
                pending = Some((name.to_string(), 0));
                i = j;
                continue;
            }
            b'{' => {
                match pending.take() {
                    Some((name, arg)) => stack.push(Frame::Cmd { name, arg, start: i }),
                    None => stack.push(Frame::Group { start: i }),
                }
            }
            b'}' => {
                // pop the innermost group/argument (environments close with \end)
                if let Some(p) = stack.iter().rposition(|f| !matches!(f, Frame::Env { .. })) {
                    let f = stack.remove(p);
                    stack.truncate(p);
                    pending = match f {
                        Frame::Cmd { name, arg, .. } => Some((name, arg + 1)),
                        _ => None,
                    };
                } else {
                    pending = None;
                }
            }
            b' ' | b'\t' | b'\n' | b'\r' => {}
            b'[' if pending.is_some() => {
                // optional argument `\sqrt[3]{x}`: skip it, keep waiting for `{`
                if let Some(c) = tex[i..upto].find(']') {
                    i += c + 1;
                    continue;
                }
            }
            _ => pending = None,
        }
        i += 1;
    }
    stack
}

/// The quote marks (`> > `) and indentation of the line containing `pos`, as Latex Suite keeps
/// them when a snippet inserts newlines. List and task markers are skipped, not repeated.
pub fn line_prefix(line_before: &str) -> (String, String) {
    let mut quotes = String::new();
    let mut rest = line_before;
    loop {
        let t = rest.trim_start_matches([' ', '\t']);
        if let Some(after) = t.strip_prefix('>') {
            let after_sp = after.strip_prefix(' ').unwrap_or(after);
            quotes.push_str(&t[..t.len() - after_sp.len()]);
            rest = after_sp;
            continue;
        }
        // list marker
        let list = ["- ", "* ", "+ "].iter().find(|m| t.starts_with(**m)).map(|m| m.len()).or_else(|| {
            let d = t.bytes().take_while(u8::is_ascii_digit).count();
            (d > 0 && matches!(t.as_bytes().get(d), Some(b'.' | b')')) && t.as_bytes().get(d + 1) == Some(&b' ')).then_some(d + 2)
        });
        if let Some(n) = list {
            rest = &t[n..];
            for m in ["[ ] ", "[x] ", "[X] "] {
                if let Some(r) = rest.strip_prefix(m) {
                    rest = r;
                }
            }
            continue;
        }
        break;
    }
    let indent: String = rest.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    (quotes, indent)
}

/// Rewrites `\n` + tabs in a snippet insertion to `\n` + the line's quote marks + its
/// indentation + those tabs, shifting tabstops accordingly.
pub fn keep_indent_and_callout(insert: &str, tabstops: &mut [(usize, Range<usize>)], line_before: &str) -> String {
    if !insert.contains('\n') {
        return insert.to_string();
    }
    let (quotes, indent) = line_prefix(line_before);
    let mut out = String::with_capacity(insert.len() + 16);
    let mut map: Vec<(usize, isize)> = Vec::new(); // (position in insert, shift after it)
    let mut shift = 0isize;
    let mut i = 0;
    let b = insert.as_bytes();
    while i < b.len() {
        if b[i] == b'\n' {
            let mut j = i + 1;
            while j < b.len() && b[j] == b'\t' {
                j += 1;
            }
            let tabs = &insert[i + 1..j];
            let rep = format!("\n{quotes}{indent}{tabs}");
            shift += rep.len() as isize - (j - i) as isize;
            map.push((i, shift));
            out.push_str(&rep);
            i = j;
        } else {
            let c = insert[i..].chars().next().expect("boundary");
            out.push(c);
            i += c.len_utf8();
        }
    }
    let shift_of = |p: usize| map.iter().rev().find(|(at, _)| p > *at).map_or(0, |(_, s)| *s);
    for (_, r) in tabstops.iter_mut() {
        *r = (r.start as isize + shift_of(r.start)) as usize..(r.end as isize + shift_of(r.end)) as usize;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t: Vec<String> = tokenize(r"\frac{a}{b} \\ \{x\} %c").into_iter().map(|t| t.text).collect();
        assert_eq!(t, vec![r"\frac", "{", "a", "}", "{", "b", "}", r"\\", r"\{", "x", r"\}", "%c"]);
    }

    #[test]
    fn brackets() {
        assert_eq!(find_matching_bracket("(a(b)c)", 0, "(", ")", false), Some(6));
        assert_eq!(find_matching_bracket("(a(b)c)", 6, "(", ")", true), Some(0));
        assert_eq!(find_matching_bracket("(a(b", 0, "(", ")", false), None);
    }

    #[test]
    fn stack() {
        let t = r"\begin{pmatrix} a & \text{b \frac{x}{y";
        let s = stack_at(t, t.len());
        assert_eq!(s, vec![
            Frame::Env { name: "pmatrix".into(), start: 0 },
            Frame::Cmd { name: "text".into(), arg: 0, start: 25 },
            Frame::Cmd { name: "frac".into(), arg: 1, start: t.rfind('{').unwrap() },
        ]);
        let t = r"\begin{a} x \end{a} \sqrt[3]{z";
        assert_eq!(stack_at(t, t.len()), vec![Frame::Cmd { name: "sqrt".into(), arg: 0, start: t.rfind('{').unwrap() }]);
        let t = r"\colorbox{red}{te";
        assert!(matches!(stack_at(t, t.len()).last(), Some(Frame::Cmd { name, arg: 1, .. }) if name == "colorbox"));
    }

    #[test]
    fn callout_prefix_is_kept() {
        assert_eq!(line_prefix("> > - [ ] text $"), ("> > ".to_string(), String::new()));
        assert_eq!(line_prefix("\t- x"), (String::new(), String::new()));
        assert_eq!(line_prefix(">   $"), ("> ".to_string(), "  ".to_string()));
        let mut ts = vec![(0, 3..3)];
        let s = keep_indent_and_callout("$$\n$$", &mut ts, "> ");
        assert_eq!(s, "$$\n> $$");
        assert_eq!(ts[0].1, 5..5);
    }
}
