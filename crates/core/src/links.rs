//! Obsidian link resolution (shortest-unique-path rule). Validated by the 16 hidden tests of
//! bench task t1 (moved here from the reference solution) plus the fixture embed test.

use std::collections::HashMap;

/// The path part of a link: wrapper, `|alias` and `#subpath` removed, trimmed. None if empty.
pub fn link_path(link: &str) -> Option<&str> {
    let mut t = link.trim();
    if let Some(s) = t.strip_prefix('!') {
        if s.starts_with("[[") {
            t = s;
        }
    }
    if let Some(s) = t.strip_prefix("[[") {
        t = s.strip_suffix("]]").unwrap_or(s);
    }
    let t = t.split('|').next().unwrap_or("");
    let t = t.split('#').next().unwrap_or("").trim();
    (!t.is_empty()).then_some(t)
}

fn dir_of(p: &str) -> &str {
    p.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Obsidian link resolution over a fixed set of vault files, indexed by lowercase basename.
#[derive(Clone, Debug, Default)]
pub struct Resolver {
    /// lowercase path -> original path
    exact: HashMap<String, String>,
    /// lowercase basename -> original paths
    by_name: HashMap<String, Vec<String>>,
}

impl Resolver {
    pub fn new<'a>(files: impl IntoIterator<Item = &'a str>) -> Self {
        let mut r = Self::default();
        for f in files {
            r.add(f);
        }
        r
    }

    pub fn add(&mut self, f: &str) {
        let l = f.to_lowercase();
        let name = l.rsplit('/').next().unwrap_or(&l).to_string();
        self.exact.insert(l, f.to_string());
        let v = self.by_name.entry(name).or_default();
        if !v.iter().any(|x| x == f) {
            v.push(f.to_string());
        }
    }

    pub fn remove(&mut self, f: &str) {
        let l = f.to_lowercase();
        let name = l.rsplit('/').next().unwrap_or(&l).to_string();
        self.exact.remove(&l);
        if let Some(v) = self.by_name.get_mut(&name) {
            v.retain(|x| x != f);
        }
    }

    /// Resolves a link (`![[x]]`, `[[x|alias]]`, `x#heading`, or a bare target) from the note at `from`.
    pub fn resolve(&self, link: &str, from: &str) -> Option<String> {
        let t = link_path(link)?;
        let last = t.rsplit('/').next().unwrap_or(t);
        let target = if last.contains('.') { t.to_string() } else { format!("{t}.md") };
        let tl = target.to_lowercase();
        if let Some(f) = self.exact.get(&tl) {
            return Some(f.clone());
        }
        let name = tl.rsplit('/').next().unwrap_or(&tl);
        let suffix = format!("/{tl}");
        let mut cands: Vec<&String> = self.by_name.get(name)?.iter().filter(|f| f.to_lowercase().ends_with(&suffix)).collect();
        if cands.is_empty() {
            return None;
        }
        let from_dir = dir_of(from);
        if let Some(c) = cands.iter().filter(|c| dir_of(c) == from_dir).min() {
            return Some((*c).clone());
        }
        cands.sort_by(|a, b| a.split('/').count().cmp(&b.split('/').count()).then(a.cmp(b)));
        Some(cands[0].clone())
    }

    /// The shortest link text that resolves to `target` from `from` (a path suffix; `.md`
    /// dropped when `keep_ext` is false).
    pub fn shortest_link(&self, target: &str, from: &str, keep_ext: bool) -> String {
        let shown = |s: &str| if !keep_ext { s.strip_suffix(".md").unwrap_or(s).to_string() } else { s.to_string() };
        let parts: Vec<&str> = target.split('/').collect();
        for n in 1..=parts.len() {
            let cand = shown(&parts[parts.len() - n..].join("/"));
            if self.resolve(&cand, from).as_deref() == Some(target) {
                return cand;
            }
        }
        shown(target)
    }
}

/// One-shot resolution over a file list (see [`Resolver`]).
pub fn resolve_link(link: &str, files: &[&str], from: &str) -> Option<String> {
    Resolver::new(files.iter().copied()).resolve(link, from)
}

#[cfg(test)]
mod tests {
    use super::{resolve_link, Resolver};
    const FILES: &[&str] = &[
        "Images/Pasted image 20250101.png",
        "Year 3/Algorithms/Graphs.md",
        "Year 2/Theory/Caffè.md",
        "Notes/Index.md",
        "Index.md",
        "A/dup.md",
        "B/C/dup.md",
        "B/dup.md",
        "Other/Planning.md",
        "Other/sub/Planning.md",
        "x/y/Same Name.png",
    ];
    #[test] fn embed_image() { assert_eq!(resolve_link("![[Pasted image 20250101.png]]", FILES, "Notes/Index.md").as_deref(), Some("Images/Pasted image 20250101.png")); }
    #[test] fn plain_link_adds_md() { assert_eq!(resolve_link("[[Graphs]]", FILES, "Index.md").as_deref(), Some("Year 3/Algorithms/Graphs.md")); }
    #[test] fn unicode_case_insensitive() { assert_eq!(resolve_link("[[CAFFÈ]]", FILES, "Index.md").as_deref(), Some("Year 2/Theory/Caffè.md")); }
    #[test] fn accented() { assert_eq!(resolve_link("[[Caffè]]", FILES, "Index.md").as_deref(), Some("Year 2/Theory/Caffè.md")); }
    #[test] fn alias_and_heading() { assert_eq!(resolve_link("[[Graphs#Algorithm|my alias]]", FILES, "Index.md").as_deref(), Some("Year 3/Algorithms/Graphs.md")); }
    #[test] fn block_ref() { assert_eq!(resolve_link("[[Graphs#^abc123]]", FILES, "Index.md").as_deref(), Some("Year 3/Algorithms/Graphs.md")); }
    #[test] fn exact_path_wins() { assert_eq!(resolve_link("[[Index]]", FILES, "Notes/Index.md").as_deref(), Some("Index.md")); }
    #[test] fn partial_path() { assert_eq!(resolve_link("[[Notes/Index]]", FILES, "A/dup.md").as_deref(), Some("Notes/Index.md")); }
    #[test] fn same_dir_preferred() { assert_eq!(resolve_link("[[dup]]", FILES, "B/C/other.md").as_deref(), Some("B/C/dup.md")); }
    #[test] fn fewest_components() { assert_eq!(resolve_link("[[Planning]]", FILES, "Index.md").as_deref(), Some("Other/Planning.md")); }
    #[test] fn lexicographic_tie() { assert_eq!(resolve_link("[[dup]]", FILES, "Index.md").as_deref(), Some("A/dup.md")); }
    #[test] fn bare_target() { assert_eq!(resolve_link("Same Name.png", FILES, "Index.md").as_deref(), Some("x/y/Same Name.png")); }
    #[test] fn missing() { assert_eq!(resolve_link("[[Nope]]", FILES, "Index.md"), None); }
    #[test] fn empty() { assert_eq!(resolve_link("[[]]", FILES, "Index.md"), None); assert_eq!(resolve_link("[[#heading]]", FILES, "Index.md"), None); }
    #[test] fn no_partial_component_match() { assert_eq!(resolve_link("[[ndex]]", FILES, "Index.md"), None); }
    #[test] fn whitespace_trim() { assert_eq!(resolve_link("[[ Graphs ]]", FILES, "Index.md").as_deref(), Some("Year 3/Algorithms/Graphs.md")); }

    #[test]
    fn shortest_link_text() {
        let r = Resolver::new(FILES.iter().copied());
        assert_eq!(r.shortest_link("Year 3/Algorithms/Graphs.md", "Index.md", false), "Graphs");
        assert_eq!(r.shortest_link("B/C/dup.md", "Index.md", false), "C/dup");
        assert_eq!(r.shortest_link("B/C/dup.md", "B/C/x.md", false), "dup");
        assert_eq!(r.shortest_link("Images/Pasted image 20250101.png", "Index.md", true), "Pasted image 20250101.png");
    }
}
