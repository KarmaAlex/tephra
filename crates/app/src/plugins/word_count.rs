//! Word count (Obsidian core plugin): words and characters of the active note in the status bar.

use std::rc::Rc;

use serde_json::{Map, Value};

use super::api::*;

pub struct WordCount {
    manifest: PluginManifest,
}

impl WordCount {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "word-count",
                name: "Word count",
                version: "0.1.0",
                description: "Words and characters of the active note (or of the selection) in the status bar.",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
        }
    }
}

/// Words (whitespace-separated runs containing a letter or digit) and characters.
pub fn count(text: &str) -> (usize, usize) {
    let words = text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count();
    (words, text.chars().count())
}

impl Plugin for WordCount {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        reg.status_item(StatusItem {
            id: "words".into(),
            text: Rc::new(|e| {
                let e = e?;
                let sel = e.doc.selection();
                let text = if sel.is_empty() { e.doc.text() } else { e.doc.slice(sel.range()) };
                let (w, c) = count(&text);
                let plural = |n: usize, s: &str| if n == 1 { format!("{n} {s}") } else { format!("{n} {s}s") };
                Some(format!("{} {}", plural(w, "word"), plural(c, "character")).into())
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts() {
        assert_eq!(super::count("a b  c\n- d $x$"), (5, 14));
        assert_eq!(super::count(""), (0, 0));
    }
}
