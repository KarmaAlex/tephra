//! Full-text search over the vault (HANDOFF §5.4), with a subset of Obsidian's query syntax:
//! words (all must match), `"phrases"`, `-negation`, `OR`, `( groups )`, and the operators
//! `path:`, `file:`, `tag:` and `content:`. Matching ignores case.
//!
//! Texts are folded once (when indexed) with [`fold`], which keeps byte offsets, so a match in
//! the folded text is also the range to highlight in the original.

use std::ops::Range;

/// Lowercases `s` char by char where that keeps the UTF-8 length (so offsets map 1:1);
/// characters whose lowercase form is longer, shorter or several chars are kept as they are.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mut low = c.to_lowercase();
        match (low.next(), low.next()) {
            (Some(l), None) if l.len_utf8() == c.len_utf8() => out.push(l),
            _ => out.push(c),
        }
    }
    out
}

/// One searchable note.
#[derive(Clone, Debug)]
pub struct SearchDoc {
    /// Vault path, e.g. `Folder/Note.md`.
    pub path: String,
    pub text: String,
    /// `fold(text)`.
    pub folded: String,
    /// `fold(path)`.
    pub path_folded: String,
    /// Tags without `#`, folded (`uni/algo`).
    pub tags: Vec<String>,
}

impl SearchDoc {
    pub fn new(path: &str, text: &str, tags: Vec<String>) -> Self {
        Self { path: path.to_string(), text: text.to_string(), folded: fold(text), path_folded: fold(path), tags: tags.iter().map(|t| fold(t.trim_start_matches('#'))).collect() }
    }

    fn file_name_folded(&self) -> &str {
        self.path_folded.rsplit('/').next().unwrap_or(&self.path_folded)
    }
}

/// A search term (values are folded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    /// Matches the content or the path.
    Word(String),
    /// A `"quoted phrase"`: matches the content or the path.
    Phrase(String),
    /// `path:` — part of the vault path.
    Path(String),
    /// `file:` — part of the file name.
    File(String),
    /// `tag:` — the tag or one of its subtags (`tag:uni` matches `uni/algo`).
    Tag(String),
    /// `content:` — the content only.
    Content(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Term(Term),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

// ---- parsing ---------------------------------------------------------------------------------

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }
    fn skip_ws(&mut self) {
        let r = self.rest();
        self.pos += r.len() - r.trim_start().len();
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    /// Text up to whitespace or a parenthesis.
    fn bare(&mut self) -> &'a str {
        let r = self.rest();
        let end = r.find(|c: char| c.is_whitespace() || c == '(' || c == ')').unwrap_or(r.len());
        self.pos += end;
        &r[..end]
    }
    /// After an opening quote: up to the closing quote (or the end).
    fn quoted(&mut self) -> &'a str {
        let r = self.rest();
        match r.find('"') {
            Some(i) => {
                self.pos += i + 1;
                &r[..i]
            }
            None => {
                self.pos += r.len();
                r
            }
        }
    }
    /// `OR` as a token of its own.
    fn at_or(&self) -> bool {
        let r = self.rest();
        r.starts_with("OR") && r[2..].chars().next().is_none_or(|c| c.is_whitespace() || c == '(' || c == ')' || c == '"')
    }

    /// alternatives separated by OR, until `)` (when nested) or the end
    fn or(&mut self, nested: bool) -> Option<Expr> {
        let mut alts = Vec::new();
        let mut cur = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some(')') => {
                    self.pos += 1;
                    if nested {
                        break;
                    }
                    continue; // stray ')'
                }
                _ => {}
            }
            if self.at_or() {
                self.pos += 2;
                alts.extend(simplify(Expr::And(std::mem::take(&mut cur))));
                continue;
            }
            if let Some(e) = self.operand() {
                cur.push(e);
            }
        }
        alts.extend(simplify(Expr::And(cur)));
        simplify(Expr::Or(alts))
    }

    fn operand(&mut self) -> Option<Expr> {
        match self.peek()? {
            '-' => {
                self.pos += 1;
                match self.peek() {
                    None => None,
                    Some(c) if c.is_whitespace() => None,
                    _ => self.operand().map(|e| Expr::Not(Box::new(e))),
                }
            }
            '(' => {
                self.pos += 1;
                self.or(true)
            }
            '"' => {
                self.pos += 1;
                let p = self.quoted();
                (!p.is_empty()).then(|| Expr::Term(Term::Phrase(fold(p))))
            }
            _ => {
                let start = self.pos;
                let word = self.bare();
                if let Some((op, after)) = word.split_once(':') {
                    let make: Option<fn(String) -> Term> = match op.to_ascii_lowercase().as_str() {
                        "path" => Some(Term::Path),
                        "file" => Some(Term::File),
                        "tag" => Some(Term::Tag),
                        "content" => Some(Term::Content),
                        _ => None,
                    };
                    if let Some(make) = make {
                        let value = if after.starts_with('"') {
                            self.pos = start + op.len() + 2; // just after the opening quote
                            self.quoted()
                        } else {
                            after
                        };
                        let value = if op.eq_ignore_ascii_case("tag") { value.trim_start_matches('#') } else { value };
                        return (!value.is_empty()).then(|| Expr::Term(make(fold(value))));
                    }
                }
                (!word.is_empty()).then(|| Expr::Term(Term::Word(fold(word))))
            }
        }
    }
}

/// One child: the child; none: dropped.
fn simplify(e: Expr) -> Option<Expr> {
    match e {
        Expr::And(mut v) | Expr::Or(mut v) if v.len() <= 1 => v.pop(),
        e => Some(e),
    }
}

/// Parses a query; None when it is blank or everything in it is ignored.
pub fn parse_query(q: &str) -> Option<Expr> {
    Parser { s: q, pos: 0 }.or(false)
}

// ---- matching ----------------------------------------------------------------------------------

fn matches(e: &Expr, d: &SearchDoc) -> bool {
    match e {
        Expr::Term(t) => match t {
            Term::Word(w) | Term::Phrase(w) => d.folded.contains(w.as_str()) || d.path_folded.contains(w.as_str()),
            Term::Content(c) => d.folded.contains(c.as_str()),
            Term::Path(p) => d.path_folded.contains(p.as_str()),
            Term::File(f) => d.file_name_folded().contains(f.as_str()),
            Term::Tag(t) => d.tags.iter().any(|x| x == t || (x.starts_with(t.as_str()) && x.as_bytes().get(t.len()) == Some(&b'/'))),
        },
        Expr::Not(e) => !matches(e, d),
        Expr::And(v) => v.iter().all(|e| matches(e, d)),
        Expr::Or(v) => v.iter().any(|e| matches(e, d)),
    }
}

/// Content terms outside any negation (what gets highlighted).
fn positive_terms<'a>(e: &'a Expr, out: &mut Vec<&'a str>) {
    match e {
        Expr::Term(Term::Word(w) | Term::Phrase(w) | Term::Content(w)) => out.push(w),
        Expr::Term(_) | Expr::Not(_) => {}
        Expr::And(v) | Expr::Or(v) => v.iter().for_each(|e| positive_terms(e, out)),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHit {
    /// 0-based line number.
    pub line: usize,
    /// Byte offset of the line start in the note.
    pub start: usize,
    /// Matched byte ranges in the note's text, sorted and merged.
    pub ranges: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: String,
    pub lines: Vec<LineHit>,
}

fn line_hits(d: &SearchDoc, terms: &[&str]) -> Vec<LineHit> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for t in terms.iter().filter(|t| !t.is_empty()) {
        ranges.extend(d.folded.match_indices(t).map(|(i, m)| i..i + m.len()));
    }
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    // group by line, walking the text once
    let bytes = d.text.as_bytes();
    let (mut line, mut line_start, mut scanned) = (0usize, 0usize, 0usize);
    let mut out: Vec<LineHit> = Vec::new();
    for r in merged {
        for (i, &b) in bytes[scanned..r.start].iter().enumerate() {
            if b == b'\n' {
                line += 1;
                line_start = scanned + i + 1;
            }
        }
        scanned = r.start;
        match out.last_mut() {
            Some(h) if h.line == line => h.ranges.push(r),
            _ => out.push(LineHit { line, start: line_start, ranges: vec![r] }),
        }
    }
    out
}

/// Every note matching `q`, sorted by file name then path, with the lines to show.
pub fn search(q: &Expr, docs: &[&SearchDoc]) -> Vec<FileHit> {
    let mut terms = Vec::new();
    positive_terms(q, &mut terms);
    let mut hits: Vec<(&SearchDoc, FileHit)> = docs.iter().filter(|d| matches(q, d)).map(|d| (*d, FileHit { path: d.path.clone(), lines: line_hits(d, &terms) })).collect();
    hits.sort_by(|(a, _), (b, _)| a.file_name_folded().cmp(b.file_name_folded()).then_with(|| a.path_folded.cmp(&b.path_folded)).then_with(|| a.path.cmp(&b.path)));
    hits.into_iter().map(|(_, h)| h).collect()
}
