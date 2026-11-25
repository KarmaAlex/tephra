//! `$…$` / `$$…$$` detection with Obsidian's rules: code blocks/spans are skipped,
//! `\$` escapes, callout `> ` prefixes are stripped from display math, `\|` is
//! unescaped in table cells.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathRange {
    /// Including the delimiters.
    pub range: Range<usize>,
    pub display: bool,
}

impl MathRange {
    /// The TeX source between the delimiters.
    pub fn inner(&self) -> Range<usize> {
        let d = if self.display { 2 } else { 1 };
        self.range.start + d..self.range.end - d
    }
}

fn escaped(b: &[u8], i: usize) -> bool {
    let mut n = 0;
    while i > n && b[i - 1 - n] == b'\\' {
        n += 1;
    }
    n % 2 == 1
}

/// A fence opener/closer at line start `ls`: up to 3 spaces, optional `>` quote markers, then
/// 3+ backticks or tildes. Returns (char, run length).
fn fence_at(b: &[u8], ls: usize) -> Option<(u8, usize)> {
    let mut k = ls;
    let mut sp = 0;
    while k < b.len() && b[k] == b' ' && sp < 4 {
        k += 1;
        sp += 1;
    }
    if sp > 3 {
        return None;
    }
    while k < b.len() && b[k] == b'>' {
        k += 1;
        while k < b.len() && b[k] == b' ' {
            k += 1;
        }
    }
    let c = *b.get(k)?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = b[k..].iter().take_while(|&&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// Length of a backtick code span starting at `i` (which holds a backtick), or None if unclosed.
pub(crate) fn code_span_end(b: &[u8], i: usize) -> Option<usize> {
    let n = b[i..].iter().take_while(|&&x| x == b'`').count();
    let mut j = i + n;
    while j < b.len() {
        if b[j] == b'`' {
            let m = b[j..].iter().take_while(|&&x| x == b'`').count();
            if m == n {
                return Some(j + m);
            }
            j += m;
        } else {
            j += 1;
        }
    }
    None
}

fn is_ws_at(s: &str, i: usize) -> bool {
    s[i..].chars().next().is_some_and(char::is_whitespace)
}
fn is_ws_before(s: &str, i: usize) -> bool {
    s[..i].chars().next_back().is_some_and(char::is_whitespace)
}

/// All math ranges in `s`, in order, never overlapping. Fenced code and code spans are skipped.
pub fn find_math(s: &str) -> Vec<MathRange> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if i == 0 || b[i - 1] == b'\n' {
            if let Some((c, n)) = fence_at(b, i) {
                // skip to the end of the closing fence line (or the end of input)
                let mut end = b.len();
                let mut p = s[i..].find('\n').map(|x| x + i);
                while let Some(nl) = p {
                    if let Some((c2, n2)) = fence_at(b, nl + 1) {
                        if c2 == c && n2 >= n {
                            end = s[nl + 1..].find('\n').map(|x| x + nl + 1).unwrap_or(b.len());
                            break;
                        }
                    }
                    p = s[nl + 1..].find('\n').map(|x| x + nl + 1);
                }
                i = end;
                continue;
            }
        }
        match b[i] {
            b'`' => {
                let n = b[i..].iter().take_while(|&&x| x == b'`').count();
                i = code_span_end(b, i).unwrap_or(i + n);
            }
            b'$' if !escaped(b, i) => {
                if b.get(i + 1) == Some(&b'$') {
                    let mut j = i + 2;
                    let mut close = None;
                    while j + 1 < b.len() {
                        if b[j] == b'$' && b[j + 1] == b'$' && !escaped(b, j) {
                            close = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    match close {
                        Some(c) => {
                            out.push(MathRange { range: i..c + 2, display: true });
                            i = c + 2;
                        }
                        None => i += 2,
                    }
                    continue;
                }
                if i + 1 < b.len() && !is_ws_at(s, i + 1) {
                    let mut j = i + 1;
                    let mut close = None;
                    while j < b.len() && b[j] != b'\n' {
                        if b[j] == b'$' && !escaped(b, j) && !is_ws_before(s, j) && !b.get(j + 1).is_some_and(u8::is_ascii_digit) {
                            close = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    if let Some(c) = close {
                        out.push(MathRange { range: i..c + 1, display: false });
                        i = c + 1;
                        continue;
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(s: &str) -> Vec<(usize, usize, bool)> {
        find_math(s).into_iter().map(|m| (m.range.start, m.range.end, m.display)).collect()
    }
    // Same cases as bench/tasks/t4-mathranges/hidden.test.ts (UTF-16 offsets there; ASCII here
    // except the unicode case, which uses byte offsets).
    #[test]
    fn cases() {
        assert_eq!(f("a $x^2$ b"), [(2, 7, false)]);
        assert_eq!(f("$$\\int_0^1 x$$"), [(0, 14, true)]);
        assert_eq!(f("t\n$$\na\\\\\nb\n$$\n"), [(2, 13, true)]);
        assert_eq!(f("costa \\$5 e \\$6"), []);
        assert_eq!(f("$a\\$b$"), [(0, 6, false)]);
        assert_eq!(f("\\\\$x$"), [(2, 5, false)]);
        assert_eq!(f("$ x$ and $y$"), [(9, 12, false)]);
        assert_eq!(f("$x $ y$"), [(0, 7, false)]);
        assert_eq!(f("$5 and $10"), []);
        assert_eq!(f("$a\nb$"), []);
        assert_eq!(f("```\n$x$\n```\n$y$"), [(12, 15, false)]);
        assert_eq!(f("~~~\n$x$\n~~~~\n$y$"), [(13, 16, false)]);
        assert_eq!(f("$a$\n```\n$b$"), [(0, 3, false)]);
        assert_eq!(f("`$x$` $y$"), [(6, 9, false)]);
        assert_eq!(f("`` ` $x$ `` $z$"), [(12, 15, false)]);
        assert_eq!(f("` $x$"), [(2, 5, false)]);
        assert_eq!(f("$`a$ `b`"), [(0, 4, false)]);
        assert_eq!(f("$$ a $b$"), [(5, 8, false)]);
        assert_eq!(f("$a$$b$"), [(0, 3, false), (3, 6, false)]);
        assert_eq!(f("| $\\mathbb{N}$ | $\\mathcal{O}(n)$ |"), [(2, 14, false), (17, 33, false)]);
        assert_eq!(f("è $x$ ù $$y$$"), [(3, 6, false), (10, 15, true)]);
        assert_eq!(f("> [!thm] T\n> $$\n> a\n> $$"), [(13, 24, true)]);
    }
}
