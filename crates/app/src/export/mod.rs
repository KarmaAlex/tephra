//! Exporting notes: Markdown → Typst (`typst`), typeset to PDF (`compile`).

pub mod compile;
pub mod typst;

/// Every note of the fixture converts and compiles (HANDOFF §7 style corpus check).
/// `cargo test --release -p obsidilight fixture_notes_export -- --ignored` (a few minutes).
#[cfg(test)]
mod fixture {
    use std::collections::HashMap;

    use super::compile::{run, Job};
    use super::typst::{Ctx, Emitter, Style};
    use crate::plugins::CalloutLook;

    #[test]
    #[ignore]
    fn fixture_notes_export() {
        let root = std::path::Path::new(&std::env::var("HOME").unwrap()).join("dev/vault-fixture");
        if !root.is_dir() {
            return eprintln!("no fixture: skipped");
        }
        let vault = crate::embeds::Vault::open(root.clone());
        let numbers = HashMap::new();
        let mut notes: Vec<String> = vault.index.files().filter(|f| f.ends_with(".md")).cloned().collect();
        notes.sort();
        let (mut ok, mut failed, mut pages) = (0, Vec::new(), 0);
        for rel in &notes {
            let text = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
            let abs = root.join(rel);
            let embed = |n: &str| vault.resolve(n, &abs);
            let e = Emitter::new(Ctx {
                numbers: &numbers,
                link_label: &|_| None,
                style: &|_, _| Style { look: CalloutLook::Obsidian, color: (60, 100, 200), label: None },
                code: &|_, _, _| None,
                callout_body: &|_, _, _, _| None,
                body_end: &|_| None,
                embed: &embed,
                no_border: false,
            });
            let body = e.blocks(&text, 0);
            let source = format!("{}\n{body}\n", crate::plugins::pdf_export::prelude(rel, &crate::plugins::pdf_export::Page::simple(10.0)));
            let fonts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            match run(Job { source, math: e.math.into_inner(), files: e.files.into_inner(), sans: fonts(&["IBM Plex Sans", "Noto Sans"]), mono: fonts(&["DejaVu Sans Mono"]) }) {
                Ok(o) => {
                    ok += 1;
                    pages += o.pages.len();
                }
                Err(err) => failed.push(format!("{rel}: {err}")),
            }
        }
        eprintln!("{ok} notes exported ({pages} pages), {} failed", failed.len());
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
