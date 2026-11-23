//! Numbering of theorem-like callouts and algorithms, reimplemented from Academic Notes'
//! observed output (its code is not used): with the vault's settings (`numbering: section`,
//! `sectionNumberSource: order`, separate counters per type) a note's H2 headings are its
//! sections, numbered in order. Inside section `s` the n-th callout of a type is `s.n`; before
//! the first H2 it is `0.n`; a note without H2 headings numbers plainly (`n`). Proofs,
//! remarks and observations are not numbered.
//!
//! Also observed (M3 follow-up, `tools/m3/observe.mjs` on a synthetic vault):
//! - `[!figure]` and `[!table]` callouts: "Figure 1.2" / "Table 1.1" (own counters, same
//!   sections); `[!subfigure]`s inside a figure are (a), (b)…
//! - Equations (`equationMode: referenced`): a `$$…$$` block is numbered "(1.2)" only if a link
//!   somewhere in the vault points at it, by its block id (`^eq1` on the line after the closing
//!   `$$`) or by a `\label{eq:x}` inside it (`[[#^eq:x]]`). Equations have their own counter per
//!   section; unreferenced ones take no number. `\tag{…}` keeps its own tag and doesn't count.
//! - References show "fig 1.2", "tab 1.1", "eq:1.2" (the formats in Academic Notes' settings).

use obsidilight_core::markdown::{callouts::flatten, parse_blocks, parse_quote, BlockKind};

/// (group, display name, short name for references, numbered?) of a callout type.
pub fn env(kind: &str) -> Option<(&'static str, &'static str, &'static str, bool)> {
    Some(match kind {
        "def" | "definition" => ("def", "Definition", "def", true),
        "asm" | "assumption" => ("def", "Assumption", "def", true),
        "thm" | "theorem" => ("thm", "Theorem", "thm", true),
        "axiom" | "axm" => ("thm", "Axiom", "thm", true),
        "hyp" | "hypothesis" => ("thm", "Hypothesis", "thm", true),
        "lem" | "lemma" => ("lem", "Lemma", "lem", true),
        "prop" | "prp" | "proposition" => ("prop", "Proposition", "prop", true),
        "cor" | "corollary" => ("cor", "Corollary", "cor", true),
        "claim" | "clm" => ("claim", "Claim", "clm", true),
        "conjecture" | "cnj" => ("claim", "Conjecture", "clm", true),
        "example" | "exm" | "ex" | "exa" => ("example", "Example", "ex", true),
        "exercise" | "exr" => ("example", "Exercise", "ex", true),
        "algorithm" | "alg" => ("algorithm", "Algorithm", "alg", true),
        "solution" | "sol" => ("solution", "Solution", "sol", false),
        "proof" | "pf" => ("proof", "Proof", "pf", false),
        "remark" | "rem" | "rmk" => ("remark", "Remark", "rem", false),
        "figure" | "fig" => ("figure", "Figure", "fig", true),
        "table" | "tab" => ("table", "Table", "tab", true),
        _ => return None,
    })
}

/// What numbering looks at in a note, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// An H2 heading (a section).
    Section,
    /// A callout: type, first and last line.
    Callout { kind: String, line: usize, end_line: usize },
    /// A display formula: first line, last line (its block id's line, if any), block id and
    /// `\label`s, explicit `\tag`, and whether anything links to it.
    Equation { line: usize, end_line: usize, ids: Vec<String>, tag: Option<String>, referenced: bool },
}

/// Formats for references (Academic Notes' `theoremFormat`, `figureFormat`…): `{type}` and
/// `{number}` are replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formats {
    pub theorem: String,
    pub figure: String,
    pub table: String,
    pub algorithm: String,
    pub equation: String,
}

impl Default for Formats {
    fn default() -> Self {
        Self { theorem: "{type} {number}".into(), figure: "fig {number}".into(), table: "tab {number}".into(), algorithm: "alg {number}".into(), equation: "eq:{number}".into() }
    }
}

impl Formats {
    fn short(&self, group: &str, short: &str, num: &str) -> String {
        let f = match group {
            "figure" => &self.figure,
            "table" => &self.table,
            "algorithm" => &self.algorithm,
            _ => &self.theorem,
        };
        f.replace("{type}", short).replace("{number}", num)
    }
}

/// Display formulas of a note (also inside callouts; not in code blocks), with their ids.
pub fn equations(text: &str) -> Vec<Item> {
    // each line without its quote markers
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r').trim_start_matches(|c: char| c == '>' || c == ' ' || c == '\t')).collect();
    let block_id = |l: &str| -> Option<String> {
        let t = l.trim_end();
        let i = t.rfind('^')?;
        let id = &t[i + 1..];
        let ok = !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') && t[..i].chars().next_back().is_none_or(char::is_whitespace);
        ok.then(|| id.to_string())
    };
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        if let Some(f) = fence {
            if l.starts_with(f) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if l.starts_with("```") || l.starts_with("~~~") {
            fence = Some(&l[..3]);
            i += 1;
            continue;
        }
        let Some(rest) = l.strip_prefix("$$") else {
            i += 1;
            continue;
        };
        // the closing $$: on this line after the opening one, or on a later line
        let (end, body, after) = match rest.find("$$") {
            Some(k) => (i, rest[..k].to_string(), rest[k + 2..].to_string()),
            None => {
                let mut body = rest.to_string();
                let mut j = i + 1;
                let mut found = None;
                while j < lines.len() {
                    if let Some(k) = lines[j].find("$$") {
                        body.push('\n');
                        body.push_str(&lines[j][..k]);
                        found = Some((j, lines[j][k + 2..].to_string()));
                        break;
                    }
                    body.push('\n');
                    body.push_str(lines[j]);
                    j += 1;
                }
                match found {
                    Some((j, after)) => (j, body, after),
                    None => break, // unclosed: not a formula
                }
            }
        };
        let mut ids = Vec::new();
        let mut end_line = end;
        if let Some(id) = block_id(&after) {
            ids.push(id);
        } else if let Some(id) = lines.get(end + 1).filter(|n| n.trim().starts_with('^')).and_then(|n| block_id(n)) {
            ids.push(id);
            end_line = end + 1;
        }
        let mut b = body.as_str();
        while let Some(k) = b.find("\\label{") {
            let r = &b[k + 7..];
            if let Some(e) = r.find('}') {
                ids.push(r[..e].trim().to_string());
            }
            b = &b[k + 7..];
        }
        let tag = ["\\tag*{", "\\tag{"].iter().find_map(|t| {
            let k = body.find(t)?;
            let r = &body[k + t.len()..];
            r.find('}').map(|e| r[..e].trim().to_string())
        });
        out.push(Item::Equation { line: i, end_line, ids, tag, referenced: false });
        i = end + 1;
    }
    out
}

/// The items of a note's text (cheap: blocks and quotes only).
pub fn items_of_text(text: &str) -> Vec<Item> {
    let mut out = callout_items(text);
    // equations, in document order (a callout before a formula inside it)
    let key = |i: &Item| match i {
        Item::Section => None,
        Item::Callout { line, .. } => Some((*line, 0)),
        Item::Equation { line, .. } => Some((*line, 1)),
    };
    let mut eqs = equations(text).into_iter().peekable();
    let mut merged = Vec::with_capacity(out.len());
    let mut last_line = 0usize;
    let mut section_lines = section_lines(text).into_iter();
    let mut next_section = section_lines.next();
    for it in out.drain(..) {
        let at = match &it {
            Item::Section => {
                let l = next_section.unwrap_or(last_line);
                next_section = section_lines.next();
                (l, 0)
            }
            other => key(other).unwrap_or((last_line, 0)),
        };
        while let Some(e) = eqs.peek() {
            if key(e).is_some_and(|k| k < at) {
                merged.push(eqs.next().expect("peeked"));
            } else {
                break;
            }
        }
        last_line = at.0;
        merged.push(it);
    }
    merged.extend(eqs);
    merged
}

fn section_lines(text: &str) -> Vec<usize> {
    let mut v = Vec::new();
    let line_of = |off: usize| text[..off.min(text.len())].matches('\n').count();
    for b in parse_blocks(text) {
        if b.kind == (BlockKind::Heading { level: 2 }) {
            v.push(line_of(b.range.start));
        }
    }
    v
}

/// Sections and callouts only.
fn callout_items(text: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let line_of = |off: usize| text[..off.min(text.len())].matches('\n').count();
    for b in parse_blocks(text) {
        match b.kind {
            BlockKind::Heading { level: 2 } => out.push(Item::Section),
            BlockKind::Quote => {
                for c in flatten(&parse_quote(text, b.range.clone())) {
                    out.push(Item::Callout { kind: c.kind.clone(), line: line_of(c.header.start), end_line: line_of(c.range.end.saturating_sub(1).max(c.header.start)) });
                }
            }
            _ => {}
        }
    }
    out
}

/// A numbered callout or equation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numbered {
    pub line: usize,
    pub end_line: usize,
    /// "Theorem 1.2", "Figure 1.1", "(a)", or an equation's "(1.2)" (empty: tagged equation,
    /// which shows its own tag).
    pub label: String,
    /// "thm 1.2", "fig 1.1", "eq:1.2" (how references show).
    pub short: String,
    /// Equations: the block id and `\label`s that references use.
    pub ids: Vec<String>,
}

/// Marks the equations whose ids `is_referenced` knows.
pub fn mark_referenced(items: &mut [Item], is_referenced: impl Fn(&str) -> bool) {
    for it in items {
        if let Item::Equation { ids, referenced, .. } = it {
            *referenced = ids.iter().any(|i| is_referenced(i));
        }
    }
}

pub fn number(items: &[Item]) -> Vec<Numbered> {
    number_with(items, &Formats::default())
}

pub fn number_with(items: &[Item], f: &Formats) -> Vec<Numbered> {
    let sectioned = items.iter().any(|i| *i == Item::Section);
    let mut section = 0usize;
    let mut counters: std::collections::HashMap<&str, usize> = Default::default();
    // the figure subfigures belong to: (lines, its number, subfigures so far)
    let mut figure: Option<(usize, usize, String, usize)> = None;
    let mut out = Vec::new();
    let num_of = |section: usize, n: usize| if sectioned { format!("{section}.{n}") } else { n.to_string() };
    for it in items {
        match it {
            Item::Section => {
                section += 1;
                counters.clear();
            }
            Item::Callout { kind, line, end_line } if kind == "subfigure" => {
                let Some((start, end, num, k)) = figure.as_mut().filter(|f| f.0 < *line && *line <= f.1) else { continue };
                let _ = start;
                let _ = end;
                let letter = (b'a' + (*k % 26) as u8) as char;
                *k += 1;
                out.push(Numbered { line: *line, end_line: *end_line, label: format!("({letter})"), short: format!("{} ({letter})", f.short("figure", "fig", num)), ids: Vec::new() });
            }
            Item::Callout { kind, line, end_line } => {
                let Some((group, name, short, true)) = env(kind) else { continue };
                let n = counters.entry(group).or_insert(0);
                *n += 1;
                let num = num_of(section, *n);
                if group == "figure" {
                    figure = Some((*line, *end_line, num.clone(), 0));
                }
                out.push(Numbered { line: *line, end_line: *end_line, label: format!("{name} {num}"), short: f.short(group, short, &num), ids: Vec::new() });
            }
            Item::Equation { line, end_line, ids, tag, referenced } => {
                if !referenced {
                    continue;
                }
                let (label, num) = match tag {
                    Some(t) => (String::new(), t.clone()),
                    None => {
                        let n = counters.entry("equation").or_insert(0);
                        *n += 1;
                        let num = num_of(section, *n);
                        (format!("({num})"), num)
                    }
                };
                out.push(Numbered { line: *line, end_line: *end_line, label, short: f.equation.replace("{number}", &num), ids: ids.clone() });
            }
        }
    }
    out
}

/// The numbered callout containing `line` (innermost), e.g. where a block id sits.
pub fn at_line(numbers: &[Numbered], line: usize) -> Option<&Numbered> {
    numbers.iter().filter(|n| n.line <= line && line <= n.end_line).max_by_key(|n| n.line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(t: &str) -> Vec<String> {
        number(&items_of_text(t)).into_iter().map(|n| n.label).collect()
    }

    #[test]
    fn sections_are_h2_headings() {
        let t = "# Title\n> [!def] a\n\n## One\n> [!prop] p\n\n> [!thm] t\n> > [!pf]\n> > x\n### Sub\n> [!thm] u\n\n## Two\n> [!lem] l\n\n> [!lemma] l2\n\n> [!algorithm|lines=true] A\n> \\STATE x\n\n> [!rem] r\n";
        assert_eq!(labels(t), vec!["Definition 0.1", "Proposition 1.1", "Theorem 1.1", "Theorem 1.2", "Lemma 2.1", "Lemma 2.2", "Algorithm 2.1"]);
    }

    #[test]
    fn no_h2_numbers_plainly() {
        let t = "### A\n> [!thm] x\n\n> [!def] d\n\n> [!def] e\n\n#### B\n> [!claim] c\n\n> [!pf]\n> > [!claim] nested\n";
        assert_eq!(labels(t), vec!["Theorem 1", "Definition 1", "Definition 2", "Claim 1", "Claim 2"]);
    }

    #[test]
    fn figures_tables_subfigures() {
        let t = "# T\n> [!figure] Before\n> x\n\n## One\n> [!figure] A\n> x\n\n> [!figure] Pair\n> > [!subfigure] L\n> > x\n>\n> > [!subfigure] R\n> > y\n\n> [!table] Costs\n> | a |\n\n> [!thm] t\n\n## Two\n> [!table]\n> | c |\n\n> [!subfigure] stray\n";
        let n = number(&items_of_text(t));
        let got: Vec<(&str, &str)> = n.iter().map(|n| (n.label.as_str(), n.short.as_str())).collect();
        assert_eq!(got, vec![("Figure 0.1", "fig 0.1"), ("Figure 1.1", "fig 1.1"), ("Figure 1.2", "fig 1.2"), ("(a)", "fig 1.2 (a)"), ("(b)", "fig 1.2 (b)"), ("Table 1.1", "tab 1.1"), ("Theorem 1.1", "thm 1.1"), ("Table 2.1", "tab 2.1")]);
    }

    #[test]
    fn equations_found_with_ids_labels_and_tags() {
        let t = "$$ a $$\n^e1\n\n```\n$$ not math $$\n```\n$$\nc = d \\label{eq:cd}\n$$\n> $$ q $$ ^inq\n$$ g \\tag{7} $$\nx $$ inline-ish $$\n$$\nunclosed";
        let eqs: Vec<(usize, usize, Vec<String>, Option<String>)> = equations(t)
            .into_iter()
            .map(|e| match e {
                Item::Equation { line, end_line, ids, tag, .. } => (line, end_line, ids, tag),
                _ => unreachable!(),
            })
            .collect();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(eqs, vec![(0, 1, s(&["e1"]), None), (6, 8, s(&["eq:cd"]), None), (9, 9, s(&["inq"]), None), (10, 10, vec![], Some("7".into()))]);
    }

    #[test]
    fn only_referenced_equations_are_numbered() {
        // the behaviour observed in Academic Notes (N3 of the synthetic vault)
        let t = "# N\n## A\n### A1\n$$ p $$\n^p1\n\n$$ q $$\n\n$$ r $$\n^r3\n\n$$ s \\tag{7} $$\n^s4\n\n$$ u $$\n^u5\n\n## B\n$$\nx \\label{eq:x}\n$$\n";
        let mut items = items_of_text(t);
        mark_referenced(&mut items, |id| ["p1", "r3", "s4", "u5", "eq:x"].contains(&id));
        let n = number_with(&items, &Formats::default());
        let got: Vec<(&str, &str)> = n.iter().map(|n| (n.label.as_str(), n.short.as_str())).collect();
        assert_eq!(got, vec![("(1.1)", "eq:1.1"), ("(1.2)", "eq:1.2"), ("", "eq:7"), ("(1.3)", "eq:1.3"), ("(2.1)", "eq:2.1")]);
        assert_eq!(at_line(&n, 4).map(|n| n.short.as_str()), Some("eq:1.1"), "the block id line belongs to its equation");
        // no H2: plain numbers
        let mut items = items_of_text("$$ z $$\n^z1\n> [!figure] f\n");
        mark_referenced(&mut items, |id| id == "z1");
        let got: Vec<String> = number(&items).into_iter().map(|n| n.short).collect();
        assert_eq!(got, vec!["eq:1", "fig 1"]);
    }

    #[test]
    fn block_ids_find_their_callout() {
        let t = "## S\n> [!lemma]\n> text\n> ^lemma1\n\nafter [[#^lemma1]]\n";
        let n = number(&items_of_text(t));
        assert_eq!(at_line(&n, 3).map(|n| n.short.as_str()), Some("lem 1.1"));
        assert!(at_line(&n, 5).is_none());
    }
}

/// Compares with what Academic Notes itself rendered for every fixture note with callouts
/// (`reports/m3-reference.jsonl`, made by `tools/m3/observe.mjs` + `tools/m3/reference.py`; gitignored as it holds note
/// text). Skipped when the report or the fixture is absent.
#[cfg(test)]
mod reference {
    use super::*;

    #[test]
    fn numbering_matches_academic_notes() {
        let report = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/m3-reference.jsonl");
        let Ok(lines) = std::fs::read_to_string(&report) else { return eprintln!("no reference report: skipped") };
        let fixture = std::path::Path::new(&std::env::var("HOME").unwrap()).join("dev/vault-fixture");
        let (mut checked, mut wrong) = (0, Vec::new());
        for l in lines.lines() {
            let d = match serde_json::from_str::<serde_json::Value>(l) { Ok(d) => d, Err(e) => { eprintln!("bad line: {e}"); continue } };
            let note = d["note"].as_str().unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(fixture.join(note)) else { continue };
            let observed: Vec<(String, String)> = d["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|s| s["callouts"].as_array().cloned().unwrap_or_default())
                .map(|c| (c["type"].as_str().unwrap_or("").to_lowercase(), c["title"].as_str().unwrap_or("").to_string()))
                .collect();
            let ours = number(&items_of_text(&text));
            let mut it = ours.iter();
            for (kind, title) in &observed {
                match env(kind) {
                    Some((_, _, _, true)) => {
                        checked += 1;
                        match it.next() {
                            // a digit right after the label must be the title's own ("1/2-approximation")
                            Some(n) if title.starts_with(&n.label) && {
                                let src_title = text.lines().nth(n.line).and_then(|h| h.split_once(']')).map(|(_, t)| t.trim_start_matches(['-', '+']).trim()).unwrap_or("");
                                let after = &title[n.label.len()..];
                                !after.starts_with(|c: char| c.is_ascii_digit() || c == '.') || src_title.starts_with(|c: char| c.is_ascii_digit())
                            } => {}
                            other => wrong.push(format!("{note}: {kind} shown as {title:?}, ours {:?}", other.map(|n| &n.label))),
                        }
                    }
                    _ => {
                        if title.split_whitespace().nth(1).is_some_and(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit())) && env(kind).is_some() {
                            wrong.push(format!("{note}: {kind} is numbered there: {title:?}"));
                        }
                    }
                }
            }
        }
        eprintln!("{checked} numbered callouts compared");
        assert!(wrong.is_empty(), "{} differences:\n{}", wrong.len(), wrong.join("\n"));
        // references: `[[#^id]]` shown as "lem 1.1"
        let mut refs = 0;
        for l in lines.lines() {
            let Ok(d) = serde_json::from_str::<serde_json::Value>(l) else { continue };
            let note = d["note"].as_str().unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(fixture.join(note)) else { continue };
            let nums = number(&items_of_text(&text));
            for r in d["sections"].as_array().into_iter().flatten().flat_map(|s| s["refs"].as_array().cloned().unwrap_or_default()) {
                let (href, shown) = (r[0].as_str().unwrap_or(""), r[1].as_str().unwrap_or(""));
                let Some(id) = href.strip_prefix("#^") else { continue };
                refs += 1;
                let line = text.lines().position(|l| l.trim_end().ends_with(&format!("^{id}")));
                let ours = line.and_then(|l| at_line(&nums, l)).map(|n| n.short.clone());
                if ours.as_deref() != Some(shown) {
                    wrong.push(format!("{note}: [[{href}]] shown as {shown:?}, ours {ours:?}"));
                }
            }
        }
        eprintln!("{refs} references compared");
        assert!(wrong.is_empty(), "{} differences:\n{}", wrong.len(), wrong.join("\n"));
        assert!(checked > 100);
    }
}

#[cfg(test)]
mod lazy {
    #[test]
    fn lazy_line_belongs_to_the_callout() {
        let t = "## S\n>[!lemma]\n>x\n^lemma1\n\n>[!pf] p\n>y\n";
        let blocks = obsidilight_core::markdown::parse_blocks(t);
        eprintln!("{:?}", blocks.iter().map(|b| (&b.kind, b.range.clone())).collect::<Vec<_>>());
        let n = super::number(&super::items_of_text(t));
        assert_eq!(super::at_line(&n, 3).map(|n| n.short.as_str()), Some("lem 1.1"));
    }
}
