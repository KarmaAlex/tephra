//! Numbering of theorem-like callouts and algorithms, reimplemented from Academic Notes'
//! observed output (its code is not used): with the vault's settings (`numbering: section`,
//! `sectionNumberSource: order`, separate counters per type) a note's H2 headings are its
//! sections, numbered in order. Inside section `s` the n-th callout of a type is `s.n`; before
//! the first H2 it is `0.n`; a note without H2 headings numbers plainly (`n`). Proofs,
//! remarks and observations are not numbered.

use obsidilight_core::index::NoteMeta;
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
}

/// The items of a note's text (cheap: blocks and quotes only).
pub fn items_of_text(text: &str) -> Vec<Item> {
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

/// The items of an indexed note (for references into other notes).
pub fn items_of_meta(m: &NoteMeta) -> Vec<Item> {
    let mut v: Vec<(usize, u8, Item)> = m.headings.iter().filter(|h| h.level == 2).map(|h| (h.line, 0, Item::Section)).collect();
    v.extend(m.callouts.iter().map(|c| (c.line, 1, Item::Callout { kind: c.kind.clone(), line: c.line, end_line: c.end_line })));
    v.sort_by_key(|(l, o, _)| (*l, *o));
    v.into_iter().map(|(_, _, i)| i).collect()
}

/// A numbered callout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numbered {
    pub line: usize,
    pub end_line: usize,
    /// "Theorem 1.2".
    pub label: String,
    /// "thm 1.2" (how references show).
    pub short: String,
}

pub fn number(items: &[Item]) -> Vec<Numbered> {
    let sectioned = items.iter().any(|i| *i == Item::Section);
    let mut section = 0usize;
    let mut counters: std::collections::HashMap<&str, usize> = Default::default();
    let mut out = Vec::new();
    for it in items {
        match it {
            Item::Section => {
                section += 1;
                counters.clear();
            }
            Item::Callout { kind, line, end_line } => {
                let Some((group, name, short, true)) = env(kind) else { continue };
                let n = counters.entry(group).or_insert(0);
                *n += 1;
                let num = if sectioned { format!("{section}.{n}") } else { n.to_string() };
                out.push(Numbered { line: *line, end_line: *end_line, label: format!("{name} {num}"), short: format!("{short} {num}") });
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
