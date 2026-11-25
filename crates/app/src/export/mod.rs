//! Exporting notes: Markdown → Typst (`typst`), typeset to PDF (`compile`).

pub mod compile;
pub mod typst;

/// Every note of the fixture converts and compiles (HANDOFF §7 style corpus check).
/// `cargo test --release -p tephra fixture_notes_export -- --ignored` (a few minutes).
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
        let (mut ok, mut failed, mut pages, mut warned) = (0, Vec::new(), 0, Vec::new());
        for rel in &notes {
            let text = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
            let abs = root.join(rel);
            let embed = |n: &str| vault.resolve(n, &abs);
            let e = Emitter::new(Ctx {
                numbers: &numbers,
                link_label: &|_| None,
                style: &|_, _| Style { look: CalloutLook::Obsidian, color: (60, 100, 200), label: None, icon: None },
                code: &|_, _, _| None,
                code_note: &|_| None,
                callout_body: &|_, _, _, _| None,
                body_end: &|_| None,
                embed: &embed,
            });
            let body = e.blocks(&text, 0);
            let source = format!("{}\n{body}\n", crate::plugins::pdf_export::prelude(rel, &crate::plugins::pdf_export::Page::simple(10.0), &Default::default()));
            let fonts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            match run(Job { source, math: e.math.into_inner(), files: e.files.into_inner(), sans: fonts(&["IBM Plex Sans", "Noto Sans"]), mono: fonts(&["DejaVu Sans Mono"]) }) {
                Ok(o) => {
                    ok += 1;
                    pages += o.pages.len();
                    if !o.warnings.is_empty() {
                        warned.push(format!("{rel}: {:?}", o.warnings));
                    }
                }
                Err(err) => failed.push(format!("{rel}: {err}")),
            }
        }
        eprintln!("{ok} notes exported ({pages} pages), {} failed, {} with Typst warnings", failed.len(), warned.len());
        assert!(failed.is_empty(), "{}", failed.join("\n"));
        assert!(warned.is_empty(), "{}", warned.join("\n"));
    }
}

/// The PDF's layout follows the reading view's: checked inside Typst (`assert`), on a real
/// compile with the export's prelude and helpers.
#[cfg(test)]
mod layout {
    use super::compile::{run, Job};
    use super::typst::Look;
    use crate::plugins::pdf_export::{prelude, Page};

    fn compile(body: &str) -> Result<Vec<String>, String> {
        // 12 pt text: one reading-view pixel is 0.75 pt
        let page = Page { font_pt: 12.0, ..Page::simple(10.0) };
        let source = format!("{}\n{body}\n", prelude("t", &page, &Look::default()));
        let fonts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        run(Job { source, math: vec![], files: vec![], sans: fonts(&["IBM Plex Sans", "Noto Sans", "DejaVu Sans"]), mono: fonts(&["DejaVu Sans Mono"]) }).map(|o| o.warnings)
    }

    #[test]
    fn lines_blanks_headings_and_tables_measure_like_the_editor() {
        let checks = r#"
#let near(a, b, what) = assert(calc.abs(a - b) < 0.05pt, message: what + ": " + repr(a) + " vs " + repr(b))
#context {
  let line = 1.618 * text.size
  near(measure(block(width: 300pt)[a #linebreak() b]).height, 2 * line, "two lines")
  near(measure(block(width: 300pt)[a

#blank_(2)

b]).height, 4 * line, "a blank line is a line of text")
  near(measure(block(width: 300pt, heading(level: 1)[X])).height, 1.618 * 1.25 * text.size, "H1: 1.618 em, line height 1.25")
  near(measure(block(width: 300pt, heading(level: 3)[X])).height, 1.318 * 1.35 * text.size, "H3")
  near(measure(block(width: 300pt, hr_())).height, 26 * u_, "a rule: 12 px, 2 px, 12 px")
  near(measure(block(width: 300pt, task_(0, true)[x])).height, line, "a task is one line")
}
#layout(size => {
  let t = measure(table_((left, right), [a], [a much longer cell], [b], [c]), width: size.width)
  assert(calc.abs(t.width - size.width) < 0.5pt, message: "a table spans the text: " + repr(t.width))
})
#{
  let w = colw_((10pt, 30pt), 80pt)
  assert(w == (20pt, 60pt), message: "spare room in proportion: " + repr(w))
  let w = colw_((10pt, 100pt, 100pt), 110pt)
  assert(w == (10pt, 50pt, 50pt), message: "narrow columns keep their width, wide ones share: " + repr(w))
}
"#;
        match compile(checks) {
            Ok(w) => assert!(w.is_empty(), "warnings: {w:?}"),
            Err(e) => panic!("{e}"),
        }
    }
}
