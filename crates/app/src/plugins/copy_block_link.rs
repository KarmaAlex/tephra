//! Copy block link (replaces the community plugin "Copy Block Link"): the editor's right-click
//! menu offers a link to the heading or block under the caret.
//! - On a heading: `[[Note#Heading]]` (and its embed `![[Note#Heading]]`).
//! - Elsewhere: `[[Note#^id]]` for the block (paragraph, list item, callout, table, formula…).
//!   An existing `^id` is reused; otherwise a new one is added, as Obsidian does: at the end of
//!   a paragraph or list item, or on its own line after a callout, table, formula or code block.

use std::rc::Rc;

use gpui_kit::{App, ClipboardItem};
use tephra_core::markdown::{parse_blocks, BlockKind};
use tephra_core::{EditKind, Selection};
use serde_json::{Map, Value};

use super::api::*;

/// What to copy for the caret at `caret`.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Heading(String),
    /// `insert`: text to add at that offset to give the block its id (None: it has one).
    Block { id: String, insert: Option<(usize, String)> },
}

fn existing_id(line: &str) -> Option<String> {
    let t = line.trim_end();
    let i = t.rfind('^')?;
    let id = &t[i + 1..];
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && t[..i].chars().next_back().is_none_or(|c| c.is_whitespace() || c == '>');
    ok.then(|| id.to_string())
}

/// A new block id: 6 lowercase letters/digits, as the plugin makes them.
pub fn new_id(seed: u64) -> String {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    (0..6)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            b"abcdefghijklmnopqrstuvwxyz0123456789"[(x % 36) as usize] as char
        })
        .collect()
}

pub fn target(text: &str, caret: usize, fresh_id: &str) -> Option<Target> {
    let blocks = parse_blocks(text);
    let ix = blocks.iter().position(|b| b.range.start <= caret && caret <= b.range.end && b.kind != BlockKind::Blank)?;
    let b = &blocks[ix];
    match b.kind {
        BlockKind::Heading { .. } => return Some(Target::Heading(text[b.content.clone()].trim().to_string())),
        BlockKind::Frontmatter | BlockKind::Blank | BlockKind::Rule => return None,
        _ => {}
    }
    let last_line_start = text[..b.range.end].rfind('\n').map_or(0, |i| i + 1).max(b.range.start);
    let last_line = &text[last_line_start..b.range.end];
    if let Some(id) = existing_id(last_line) {
        return Some(Target::Block { id, insert: None });
    }
    // a block id on the line right after (callouts, tables, formulas)
    let next = blocks.get(ix + 1).filter(|n| n.range.start == b.range.end + 1);
    if let Some(id) = next.and_then(|n| existing_id(&text[n.range.clone()]).filter(|_| text[n.range.clone()].trim().starts_with('^'))) {
        return Some(Target::Block { id, insert: None });
    }
    let own_line = matches!(b.kind, BlockKind::Quote | BlockKind::Table | BlockKind::MathBlock | BlockKind::Code { .. });
    let insert = if own_line { (b.range.end, format!("\n^{fresh_id}")) } else { (b.range.end, format!(" ^{fresh_id}")) };
    Some(Target::Block { id: fresh_id.to_string(), insert: Some(insert) })
}

pub struct CopyBlockLink {
    manifest: PluginManifest,
}

impl CopyBlockLink {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "copy-block-link",
                name: "Copy block link",
                version: "0.1.0",
                description: "Right-click a heading or block to copy a link (or embed) to it; blocks get a ^id when needed.",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

fn copy(cx: &mut App, s: String) {
    if std::env::var_os("TEPHRA_SCRIPT").is_some() {
        println!("COPIED {s}");
    }
    cx.write_to_clipboard(ClipboardItem::new_string(s));
}

impl Plugin for CopyBlockLink {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        reg.editor_menu(Rc::new(|e, _cx| {
            let text = e.doc.text();
            let caret = e.doc.selection().head;
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) ^ caret as u64;
            let Some(t) = target(&text, caret, &new_id(seed)) else { return Vec::new() };
            let (note, from) = {
                let v = e.vault.borrow();
                let rel = v.relative(&e.path);
                (v.index.shortest_link(&rel, &rel), rel)
            };
            let note = if note.is_empty() { from.trim_end_matches(".md").rsplit('/').next().unwrap_or("").to_string() } else { note };
            let entry = |label: &str, link: String, insert: Option<(usize, String)>| MenuEntry {
                label: label.to_string(),
                run: Rc::new(move |a: &mut ActionCtx| {
                    if let (Some((at, s)), Some(ed)) = (insert.clone(), a.editor.clone()) {
                        ed.update(a.cx, |e, cx| {
                            let sel = e.doc.selection();
                            e.edit(at..at, &s, EditKind::Other, cx);
                            e.set_selection(Selection::new(sel.anchor, sel.head), false, cx);
                        });
                    }
                    copy(a.cx, link.clone());
                }),
            };
            match t {
                Target::Heading(h) => vec![entry("Copy link to heading", format!("[[{note}#{h}]]"), None), entry("Copy heading embed", format!("![[{note}#{h}]]"), None)],
                Target::Block { id, insert } => vec![entry("Copy link to block", format!("[[{note}#^{id}]]"), insert.clone()), entry("Copy block embed", format!("![[{note}#^{id}]]"), insert)],
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::{new_id, target, Target};

    #[test]
    fn headings_paragraphs_lists_and_quoted_blocks() {
        let t = "# Title\n## Sez\npara one\npara two\n- item\n- other ^have\n\n> [!thm] T\n> body\n\n$$\nx\n$$\n^eq1\n";
        assert_eq!(target(t, 10, "abc123"), Some(Target::Heading("Sez".into())));
        let p = t.find("one").unwrap();
        let end = t.find("two").unwrap() + 3;
        assert_eq!(target(t, p, "abc123"), Some(Target::Block { id: "abc123".into(), insert: Some((end, " ^abc123".into())) }));
        assert_eq!(target(t, t.find("other").unwrap(), "x"), Some(Target::Block { id: "have".into(), insert: None }));
        let q_end = t.find("> body").unwrap() + 6;
        assert_eq!(target(t, t.find("body").unwrap(), "q1w2e3"), Some(Target::Block { id: "q1w2e3".into(), insert: Some((q_end, "\n^q1w2e3".into())) }));
        assert_eq!(target(t, t.find("\nx\n").unwrap() + 1, "n"), Some(Target::Block { id: "eq1".into(), insert: None }), "id on the line after a formula");
        let id = new_id(42);
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_ne!(new_id(42), new_id(43));
    }
}
