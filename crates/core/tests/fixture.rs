//! Runs against a local vault fixture, never a real vault.
//! The fixture-wide tests skip (pass) when no fixture is present. Path: $VAULT or
//! ~/dev/vault-fixture. The last test builds its own tiny vault (with callouts) and always
//! runs. Reports only counts and paths, never note content.

use std::path::{Path, PathBuf};

use tephra_core::file::{self, SaveOutcome};
use tephra_core::markdown::{find_math, parse_blocks, parse_inline, BlockKind, Inline, InlineKind};
use tephra_core::Document;

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
    let tmp = std::env::temp_dir().join(format!("tephra-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let ns = notes(&root);
    assert!(ns.len() >= 100, "expected a sizeable fixture, found {} notes", ns.len());
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

/// Every `![[…]]` in the fixture resolves.
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
                if tephra_core::links::resolve_link(target, &refs, &rel).is_none() {
                    missing.push(format!("{rel}: line {}", text[..e.range.start].matches('\n').count() + 1));
                }
            }
        }
    }
    eprintln!("{total} embeds, {} unresolved", missing.len());
    assert!(total >= 100, "expected a sizeable fixture, found {total} embeds");
    assert!(missing.is_empty(), "unresolved embeds:\n{}", missing.join("\n"));
}

/// Vault index: builds within the performance budget (see AGENTS.md), and rename plans keep every link pointing at the
/// same (moved) file. Runs in memory; the fixture is not modified.
#[test]
fn index_build_and_rename_plans() {
    use tephra_core::index::{apply_edits, parse_note, VaultIndex};
    use tephra_core::links::Resolver;
    let Some(root) = fixture() else { return };
    let t = std::time::Instant::now();
    // (block ids referenced by same-note links must be found)
    let idx = VaultIndex::build(&root);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("index build: {ms:.0} ms");
    assert!(ms < 2000.0, "full index took {ms:.0} ms (budget 2 s)");
    let notes: Vec<_> = idx.notes().collect();
    // every Markdown file outside dot-folders is a note (the fixture grows as it is used)
    fn md_files(d: &std::path::Path) -> usize {
        std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') { 0 } else if p.is_dir() { md_files(&p) } else { usize::from(name.ends_with(".md")) }
        }).sum()
    }
    assert!(notes.len() >= 100);
    assert_eq!(notes.len(), md_files(&root));
    let links: usize = notes.iter().map(|n| n.links.len()).sum();
    let unresolved: Vec<String> = notes.iter().flat_map(|n| n.links.iter().filter(|l| l.resolved.is_none()).map(move |l| format!("{}: line {}", n.path, l.line + 1))).collect();
    let callouts: usize = notes.iter().map(|n| n.callouts.len()).sum();
    eprintln!("{links} links, {} unresolved, {callouts} callouts", unresolved.len());
    // a link may dangle (notes not written yet), but never when its target file exists
    let stems: std::collections::HashSet<String> = idx
        .files()
        .map(|f| f.rsplit('/').next().unwrap_or(f).trim_end_matches(".md").to_lowercase())
        .collect();
    let wrong: Vec<String> = notes
        .iter()
        .flat_map(|n| n.links.iter().filter(|l| l.resolved.is_none()).map(move |l| (n.path.clone(), l)))
        .filter(|(_, l)| {
            let t = l.target.split(['#', '|']).next().unwrap_or("").trim();
            !t.is_empty() && stems.contains(&t.rsplit('/').next().unwrap_or(t).trim_end_matches(".md").to_lowercase())
        })
        .map(|(p, l)| format!("{p}: line {}", l.line + 1))
        .collect();
    assert!(wrong.is_empty(), "links to existing files left unresolved:\n{}", wrong.join("\n"));
    assert!(callouts > 0, "callouts went missing");

    // rename a sample of targets (notes and images) and check every link still resolves
    let mut targets: Vec<String> = idx.files().filter(|f| !idx.backlinks(f).is_empty()).cloned().collect();
    targets.sort();
    let sample: Vec<String> = targets.iter().step_by((targets.len() / 25).max(1)).cloned().collect();
    for old in sample {
        let (dir, name) = old.rsplit_once('/').unwrap_or(("", &old));
        let new = format!("{}Renamed {name}", if dir.is_empty() { String::new() } else { format!("{dir}/") });
        let moves = vec![(old.clone(), new.clone())];
        let plan = idx.rename_plan(&moves);
        let after = Resolver::new(idx.files().map(|f| if *f == old { new.as_str() } else { f.as_str() }));
        for edit in &plan {
            let src = if edit.note == new { old.clone() } else { edit.note.clone() };
            let text = std::fs::read_to_string(root.join(&src)).unwrap();
            let edited = apply_edits(&text, &edit.edits);
            let before = idx.note(&src).unwrap();
            let now = parse_note(&edit.note, &edited);
            assert_eq!(now.links.len(), before.links.len());
            for (b, a) in before.links.iter().zip(&now.links) {
                let want = b.resolved.as_ref().map(|r| if *r == old { new.clone() } else { r.clone() });
                assert_eq!(after.resolve(&a.target, &edit.note), want, "{} -> {}: link {:?} in {}", old, new, a.target, edit.note);
            }
        }
        // every backlink of the renamed file got an edit
        let edited_notes: std::collections::HashSet<&str> = plan.iter().map(|e| e.note.as_str()).collect();
        for bl in idx.backlinks(&old) {
            if bl.source == old {
                continue; // same-note links ([[#^id]]) have no path to rewrite
            }
            assert!(edited_notes.contains(bl.source.as_str()), "{old}: backlink in {} not updated", bl.source);
        }
    }
}

/// A tiny generated vault with callouts, embeds and links, so those are checked exactly even
/// when no external fixture is present. Runs in a temp dir; nothing else is touched.
#[test]
fn synthetic_vault_indexes_callouts_embeds_and_renames() {
    use tephra_core::index::{apply_edits, parse_note, VaultIndex};
    use tephra_core::links::Resolver;
    let root = std::env::temp_dir().join(format!("tephra-synthetic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::create_dir_all(root.join("Sub")).unwrap();
    std::fs::write(root.join("A.md"), "# Title\nsee [[B#Sec|here]], ![[img.png]] and ![[B]], plus [[Missing]] #tag\n> [!thm|x=1] Theorem\n> body ^blk\n").unwrap();
    std::fs::write(root.join("Sub/B.md"), "## Sec\n[[A]]\n> [!note] Remark\n> text\n").unwrap();
    std::fs::write(root.join("img.png"), "PNG").unwrap();

    let idx = VaultIndex::build(&root);
    let notes: Vec<_> = idx.notes().collect();
    assert_eq!(notes.len(), 2);
    let a = idx.note("A.md").unwrap();
    // callouts are indexed with kind, meta, title and block id
    assert_eq!(a.callouts.len(), 1);
    assert_eq!((a.callouts[0].kind.as_str(), a.callouts[0].meta.as_str(), a.callouts[0].title.as_str()), ("thm", "x=1", "Theorem"));
    assert_eq!(a.block_ids, vec![("blk".to_string(), 3)]);
    assert_eq!(a.tags, vec!["tag"]);
    let b = idx.note("Sub/B.md").unwrap();
    assert_eq!(b.callouts.len(), 1);
    assert_eq!((b.callouts[0].kind.as_str(), b.callouts[0].title.as_str()), ("note", "Remark"));
    assert_eq!(notes.iter().map(|n| n.callouts.len()).sum::<usize>(), 2, "every callout counted exactly once");
    // links and embeds resolve (an embed is a link with embed: true)
    let r: Vec<_> = a.links.iter().map(|l| (l.target.as_str(), l.resolved.as_deref())).collect();
    assert_eq!(r, vec![("B#Sec", Some("Sub/B.md")), ("img.png", Some("img.png")), ("B", Some("Sub/B.md")), ("Missing", None)]);
    // both the link and the embed point at B
    assert_eq!(idx.backlinks("Sub/B.md").len(), 2);
    assert!(idx.backlinks("Sub/B.md").iter().all(|b| b.source == "A.md"));

    // a rename plan rewrites every link (and embed) that pointed at the moved file
    let old = "Sub/B.md";
    let new = "Sub/Renamed.md";
    let plan = idx.rename_plan(&[(old.into(), new.into())]);
    let after = Resolver::new(idx.files().map(|f| if *f == old { new } else { f }));
    for edit in &plan {
        let src = if edit.note == new { old.to_string() } else { edit.note.clone() };
        let text = std::fs::read_to_string(root.join(&src)).unwrap();
        let edited = apply_edits(&text, &edit.edits);
        let before = idx.note(&src).unwrap();
        let now = parse_note(&edit.note, &edited);
        assert_eq!(now.links.len(), before.links.len());
        for (b, a) in before.links.iter().zip(&now.links) {
            let want = b.resolved.as_ref().map(|r| if *r == old { new.to_string() } else { r.clone() });
            assert_eq!(after.resolve(&a.target, &edit.note), want, "link {:?} in {}", a.target, edit.note);
        }
    }
    let edited_notes: std::collections::HashSet<&str> = plan.iter().map(|e| e.note.as_str()).collect();
    assert!(edited_notes.contains("A.md"), "the backlink source got an edit");

    let _ = std::fs::remove_dir_all(&root);
}
