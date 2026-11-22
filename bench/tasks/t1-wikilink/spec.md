Write the Rust file `src/lib.rs` for a crate (edition 2021, NO external crates) that resolves Obsidian wikilinks to vault file paths.

```rust
/// `link` is the raw link text, possibly with wrapper: one of
///   "![[target]]", "[[target]]", or just "target".
/// The target may contain an alias "target|alias" and/or a subpath "target#heading" or "target#^blockid";
/// both alias and subpath must be ignored for resolution.
/// `files` are all vault files as vault-relative paths using '/' separators (e.g. "Images/Pasted image 20250101.png").
/// `from` is the vault-relative path of the note containing the link.
/// Returns the vault-relative path of the resolved file, or None.
pub fn resolve_link(link: &str, files: &[&str], from: &str) -> Option<String>;
```

Resolution rules (Obsidian's shortest-unique-path rule):
1. Strip the wrapper, then cut the target at the first '|' and then at the first '#'. Trim surrounding whitespace. Empty target -> None.
2. If the target has no file extension (the last path component has no '.'), append ".md".
3. Comparisons are case-insensitive (use Unicode-aware `to_lowercase()`); paths may contain non-ASCII characters such as "Borůvka" or "calcolabilità". Never index strings by byte offsets that could split a UTF-8 character.
4. If a file's path equals the target exactly (case-insensitively), return it.
5. Otherwise the candidates are files whose path ends with "/" + target (path-component suffix match, case-insensitive).
6. No candidates -> None. One candidate -> it.
7. Several candidates: prefer a candidate in the same directory as `from`; otherwise the candidate with the fewest path components; ties broken by lexicographic order of the original path.
8. Return the path exactly as it appears in `files` (original case).
