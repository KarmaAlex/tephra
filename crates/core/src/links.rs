//! Obsidian link resolution (shortest-unique-path rule). Validated by the 16 hidden tests of
//! bench task t1 (moved here from the reference solution) plus the fixture embed test.

/// Resolves a link (`![[x]]`, `[[x|alias]]`, `x#heading`, or a bare target) to a vault path.
/// `files` are vault-relative paths with `/` separators; `from` is the linking note's path.
pub fn resolve_link(link: &str, files: &[&str], from: &str) -> Option<String> {
    let mut t = link.trim();
    if let Some(s) = t.strip_prefix('!') { if s.starts_with("[[") { t = s; } }
    if let Some(s) = t.strip_prefix("[[") { t = s.strip_suffix("]]").unwrap_or(s); }
    let t = t.split('|').next().unwrap_or("");
    let t = t.split('#').next().unwrap_or("").trim();
    if t.is_empty() { return None; }
    let last = t.rsplit('/').next().unwrap_or(t);
    let target = if last.contains('.') { t.to_string() } else { format!("{t}.md") };
    let tl = target.to_lowercase();
    if let Some(f) = files.iter().find(|f| f.to_lowercase() == tl) { return Some(f.to_string()); }
    let suffix = format!("/{tl}");
    let mut cands: Vec<&str> = files.iter().copied().filter(|f| f.to_lowercase().ends_with(&suffix)).collect();
    if cands.is_empty() { return None; }
    let dir = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let from_dir = dir(from);
    if let Some(c) = cands.iter().filter(|c| dir(c) == from_dir).min() { return Some(c.to_string()); }
    cands.sort_by(|a, b| a.split('/').count().cmp(&b.split('/').count()).then(a.cmp(b)));
    Some(cands[0].to_string())
}

#[cfg(test)]
mod tests {
    use super::resolve_link;
    const FILES: &[&str] = &[
        "Images/Pasted image 20250101.png",
        "Terzo anno/Algoritmi/Borůvka.md",
        "Secondo anno/Calcolabilità e complessità/calcolabilità.md",
        "Notes/Index.md",
        "Index.md",
        "A/dup.md",
        "B/C/dup.md",
        "B/dup.md",
        "Altro/Thesis.md",
        "Altro/sub/Thesis.md",
        "x/y/Same Name.png",
    ];
    #[test] fn embed_image() { assert_eq!(resolve_link("![[Pasted image 20250101.png]]", FILES, "Notes/Index.md").as_deref(), Some("Images/Pasted image 20250101.png")); }
    #[test] fn plain_link_adds_md() { assert_eq!(resolve_link("[[Borůvka]]", FILES, "Index.md").as_deref(), Some("Terzo anno/Algoritmi/Borůvka.md")); }
    #[test] fn unicode_case_insensitive() { assert_eq!(resolve_link("[[BORŮVKA]]", FILES, "Index.md").as_deref(), Some("Terzo anno/Algoritmi/Borůvka.md")); }
    #[test] fn accented() { assert_eq!(resolve_link("[[Calcolabilità]]", FILES, "Index.md").as_deref(), Some("Secondo anno/Calcolabilità e complessità/calcolabilità.md")); }
    #[test] fn alias_and_heading() { assert_eq!(resolve_link("[[Borůvka#Algoritmo|il mio alias]]", FILES, "Index.md").as_deref(), Some("Terzo anno/Algoritmi/Borůvka.md")); }
    #[test] fn block_ref() { assert_eq!(resolve_link("[[Borůvka#^abc123]]", FILES, "Index.md").as_deref(), Some("Terzo anno/Algoritmi/Borůvka.md")); }
    #[test] fn exact_path_wins() { assert_eq!(resolve_link("[[Index]]", FILES, "Notes/Index.md").as_deref(), Some("Index.md")); }
    #[test] fn partial_path() { assert_eq!(resolve_link("[[Notes/Index]]", FILES, "A/dup.md").as_deref(), Some("Notes/Index.md")); }
    #[test] fn same_dir_preferred() { assert_eq!(resolve_link("[[dup]]", FILES, "B/C/other.md").as_deref(), Some("B/C/dup.md")); }
    #[test] fn fewest_components() { assert_eq!(resolve_link("[[Thesis]]", FILES, "Index.md").as_deref(), Some("Altro/Thesis.md")); }
    #[test] fn lexicographic_tie() { assert_eq!(resolve_link("[[dup]]", FILES, "Index.md").as_deref(), Some("A/dup.md")); }
    #[test] fn bare_target() { assert_eq!(resolve_link("Same Name.png", FILES, "Index.md").as_deref(), Some("x/y/Same Name.png")); }
    #[test] fn missing() { assert_eq!(resolve_link("[[Nope]]", FILES, "Index.md"), None); }
    #[test] fn empty() { assert_eq!(resolve_link("[[]]", FILES, "Index.md"), None); assert_eq!(resolve_link("[[#heading]]", FILES, "Index.md"), None); }
    #[test] fn no_partial_component_match() { assert_eq!(resolve_link("[[ndex]]", FILES, "Index.md"), None); }
    #[test] fn whitespace_trim() { assert_eq!(resolve_link("[[ Borůvka ]]", FILES, "Index.md").as_deref(), Some("Terzo anno/Algoritmi/Borůvka.md")); }
}
