//! Runs against the private vault fixture (HANDOFF §2.2), never the real vault.
//! Skips (passes) when the fixture is absent. Path: $VAULT or ~/dev/vault-fixture.
//! Reports only counts and paths, never note content.

use std::path::{Path, PathBuf};

use obsidilight_core::file::{self, SaveOutcome};
use obsidilight_core::markdown::{find_math, parse_blocks, parse_inline, BlockKind, Inline, InlineKind};
use obsidilight_core::Document;

fn fixture() -> Option<PathBuf> {
    let p = std::env::var_os("VAULT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("dev/vault-fixture")))?;
    p.join(".obsidian").is_dir().then_some(p)
}

fn notes(root: &Path) -> Vec<PathBuf> {
    fn rec(d: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(d).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() {
                rec(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    rec(root, &mut v);
    v
}

fn count_math(v: &[Inline]) -> usize {
    v.iter().map(|n| usize::from(matches!(n.kind, InlineKind::Math { .. })) + count_math(&n.children)).sum()
}

#[test]
fn roundtrip_blocks_and_math_over_fixture() {
    let Some(root) = fixture() else {
        eprintln!("fixture not found; skipping");
        return;
    };
    let tmp = std::env::temp_dir().join(format!("obsidilight-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let ns = notes(&root);
    assert!(ns.len() >= 200, "expected the full fixture, found {} notes", ns.len());
    let (mut total_math, mut mismatches) = (0usize, Vec::new());
    for (i, path) in ns.iter().enumerate() {
        let rel = path.strip_prefix(&root).unwrap().display().to_string();
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{rel}: not UTF-8"));
        // 1. in-memory round trip
        assert!(Document::new(&text).text().as_bytes() == bytes.as_slice(), "{rel}: round trip differs");
        // 2. load + unchanged save through the file layer (on a copy)
        let copy = tmp.join(format!("{i}.md"));
        std::fs::write(&copy, &bytes).unwrap();
        let l = file::load(&copy).unwrap();
        assert!(matches!(file::save(&copy, &l.text, Some(l.disk)).unwrap(), SaveOutcome::Unchanged(_)), "{rel}");
        assert!(std::fs::read(&copy).unwrap() == bytes, "{rel}: bytes changed by save");
        // 3. blocks cover every line exactly once, in order
        let blocks = parse_blocks(&text);
        let mut prev_end = 0usize;
        for (k, b) in blocks.iter().enumerate() {
            if k > 0 {
                let gap = &text[prev_end..b.range.start];
                assert!(gap == "\n" || gap == "\r\n", "{rel}: gap {gap:?} before block {k}");
            }
            prev_end = b.range.end;
        }
        assert_eq!(prev_end, text.len(), "{rel}: blocks end early");
        // 4. math found block-by-block == whole-document scan
        let mut by_blocks = 0;
        for b in &blocks {
            match b.kind {
                BlockKind::MathBlock => by_blocks += 1,
                BlockKind::Code { .. } | BlockKind::Frontmatter | BlockKind::Blank | BlockKind::Rule => {}
                _ => by_blocks += count_math(&parse_inline(&text, b.content.clone())),
            }
        }
        let whole = find_math(&text).len();
        total_math += whole;
        if by_blocks != whole {
            mismatches.push(format!("{rel}: blocks {by_blocks} vs whole {whole}"));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!("{} notes, {} formulas", ns.len(), total_math);
    assert!(mismatches.is_empty(), "math count mismatches:\n{}", mismatches.join("\n"));
}

fn all_files(root: &Path) -> Vec<String> {
    fn rec(root: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                rec(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut v = Vec::new();
    rec(root, root, &mut v);
    v
}

fn embeds<'a>(v: &'a [Inline], out: &mut Vec<&'a Inline>) {
    for n in v {
        if n.kind == (InlineKind::WikiLink { embed: true }) {
            out.push(n);
        }
        embeds(&n.children, out);
    }
}

/// HANDOFF §7: every `![[…]]` in the fixture resolves.
#[test]
fn every_embed_resolves() {
    let Some(root) = fixture() else { return };
    let files = all_files(&root);
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let (mut total, mut missing) = (0, Vec::new());
    for path in notes(&root) {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&path).unwrap();
        for b in parse_blocks(&text) {
            if matches!(b.kind, BlockKind::Code { .. } | BlockKind::MathBlock | BlockKind::Frontmatter) {
                continue;
            }
            let inl = parse_inline(&text, b.content.clone());
            let mut found = Vec::new();
            embeds(&inl, &mut found);
            for e in found {
                total += 1;
                let target = e.link_target(&text);
                if obsidilight_core::links::resolve_link(target, &refs, &rel).is_none() {
                    missing.push(format!("{rel}: line {}", text[..e.range.start].matches('\n').count() + 1));
                }
            }
        }
    }
    eprintln!("{total} embeds, {} unresolved", missing.len());
    assert!(total >= 1000, "expected the fixture's ~1,283 embeds, found {total}");
    assert!(missing.is_empty(), "unresolved embeds:\n{}", missing.join("\n"));
}
