Write the Rust file `src/lib.rs` for a crate (edition 2021, NO external crates) that parses YAML frontmatter and builds a heading tree from a Markdown note.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,            // 1..=6
    pub text: String,         // heading text, trimmed, closing #-sequence removed
    pub line: usize,          // 0-based line number in the ORIGINAL input
    pub children: Vec<Heading>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Raw text between the opening and closing `---` lines (without those lines, without the final newline
    /// before the closing `---`). None if there is no frontmatter.
    pub frontmatter: Option<String>,
    /// Byte offset in the input where the body starts (just after the closing `---` line and its newline; 0 if none).
    pub body_offset: usize,
    /// Top-level headings (forest).
    pub headings: Vec<Heading>,
}

pub fn parse(md: &str) -> Parsed;
```

Rules:
- Lines are separated by "\n"; a trailing "\r" on a line must be ignored.
- Frontmatter exists only if the very first line is exactly `---` and a later line is exactly `---`. Otherwise None and body_offset 0. An empty frontmatter (`---` immediately followed by `---`) is Some("").
- ATX headings only: 1-6 '#' characters at the start of the line (up to 3 leading spaces allowed), followed by a space or tab or end of line. `#tag` (no space) is NOT a heading. "####### x" (7 #) is not a heading.
- Strip an optional closing sequence: trailing spaces, then a run of '#' that is preceded by a space (e.g. "## Foo ##" -> "Foo"; "## C#" stays "C#").
- Ignore headings inside fenced code blocks: a fence opens with a line starting (after up to 3 spaces) with ``` or ~~~ (3 or more), and closes with a line starting with the same character repeated at least as many times. An unclosed fence runs to the end of the document.
- Headings inside frontmatter are ignored.
- Tree: a heading becomes a child of the closest preceding heading with a smaller level; if there is none, it is top-level. Levels may skip (an h3 directly under an h1 is a child of that h1).
