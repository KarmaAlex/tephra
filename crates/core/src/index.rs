//! The vault index: per-note metadata (headings, links, block ids, tags, callouts), link
//! resolution, backlinks, and rename plans that keep links pointing at moved files.
//!
//! Pure data structure: the caller feeds it file contents (initial scan, watcher events,
//! saves). Concurrency stays with the caller (the app updates it on its main thread).

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::links::{link_path, Resolver};
use crate::markdown::callouts::{flatten, parse_quote};
use crate::markdown::{parse_blocks, parse_inline, BlockKind, Inline, InlineKind};
use crate::search::{self, Expr, FileHit, SearchDoc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingRef {
    pub level: u8,
    pub text: String,
    pub line: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRef {
    /// Link text without wrapper and alias, e.g. `note#heading` or `Pasted image 1.png`.
    pub target: String,
    pub embed: bool,
    /// Markdown `[t](url)` link rather than a wikilink.
    pub markdown: bool,
    /// Byte range of the whole link in the note.
    pub range: Range<usize>,
    /// Byte range of the path part (what a rename rewrites).
    pub path_range: Range<usize>,
    pub line: usize,
    pub resolved: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalloutRef {
    pub kind: String,
    pub meta: String,
    pub title: String,
    pub line: usize,
    /// Last line of the callout (its body included).
    pub end_line: usize,
    pub depth: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteMeta {
    pub path: String,
    pub frontmatter: Option<String>,
    pub headings: Vec<HeadingRef>,
    pub links: Vec<LinkRef>,
    pub block_ids: Vec<(String, usize)>,
    pub tags: Vec<String>,
    pub callouts: Vec<CalloutRef>,
    pub words: usize,
}

fn line_of(text: &str, off: usize) -> usize {
    text[..off].matches('\n').count()
}

fn walk<'a>(v: &'a [Inline], f: &mut impl FnMut(&'a Inline)) {
    for n in v {
        f(n);
        walk(&n.children, f);
    }
}

/// Parses one note's metadata (links unresolved).
pub fn parse_note(path: &str, text: &str) -> NoteMeta {
    let mut m = NoteMeta { path: path.to_string(), ..Default::default() };
    for b in parse_blocks(text) {
        match &b.kind {
            BlockKind::Frontmatter => {
                let fm = &text[b.content.clone()];
                m.tags.extend(frontmatter_tags(fm));
                m.frontmatter = Some(fm.to_string());
            }
            BlockKind::Code { .. } | BlockKind::MathBlock | BlockKind::Blank | BlockKind::Rule => {}
            kind => {
                if let BlockKind::Heading { level } = kind {
                    m.headings.push(HeadingRef { level: *level, text: text[b.content.clone()].trim().to_string(), line: line_of(text, b.range.start), offset: b.range.start });
                }
                if *kind == BlockKind::Quote {
                    for c in flatten(&parse_quote(text, b.range.clone())) {
                        m.callouts.push(CalloutRef { kind: c.kind.clone(), meta: c.meta.clone(), title: text[c.title.clone()].to_string(), line: line_of(text, c.header.start), end_line: line_of(text, c.range.end.saturating_sub(1).max(c.header.start)), depth: c.depth });
                    }
                }
                let inl = parse_inline(text, b.content.clone());
                walk(&inl, &mut |n| match n.kind {
                    InlineKind::WikiLink { embed } => {
                        let open = if embed { 3 } else { 2 };
                        let body = n.range.start + open..n.range.end - 2;
                        let raw = &text[body.clone()];
                        let path_len = raw.find(['#', '|']).unwrap_or(raw.len());
                        m.links.push(LinkRef {
                            target: raw.split('|').next().unwrap_or(raw).to_string(),
                            embed,
                            markdown: false,
                            range: n.range.clone(),
                            path_range: body.start..body.start + path_len,
                            line: line_of(text, n.range.start),
                            resolved: None,
                        });
                    }
                    InlineKind::Link | InlineKind::Image => {
                        let url = n.link_target(text);
                        if !url.is_empty() && !url.contains("://") && !url.starts_with("mailto:") && !url.starts_with('#') {
                            let start = n.inner.end + text[n.inner.end..n.range.end].find(url).unwrap_or(0);
                            let decoded = url.replace("%20", " ");
                            m.links.push(LinkRef {
                                target: decoded,
                                embed: n.kind == InlineKind::Image,
                                markdown: true,
                                range: n.range.clone(),
                                path_range: start..start + url.find('#').unwrap_or(url.len()),
                                line: line_of(text, n.range.start),
                                resolved: None,
                            });
                        }
                    }
                    InlineKind::BlockId => m.block_ids.push((text[n.range.start + 1..n.range.end].to_string(), line_of(text, n.range.start))),
                    InlineKind::Tag => m.tags.push(text[n.range.start + 1..n.range.end].to_string()),
                    _ => {}
                });
                m.words += text[b.content.clone()].split_whitespace().count();
            }
        }
    }
    m
}

/// Tags listed in YAML frontmatter (`tags:` or `tag:`), as Obsidian reads them:
/// `tags: [a, b]`, `tags: a, b`, `tags: a b`, or a `- a` list on the following lines.
pub fn frontmatter_tags(fm: &str) -> Vec<String> {
    frontmatter_list(fm, &["tags", "tag"]).into_iter().map(|t| t.trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect()
}

/// The values of a list property (any of `keys`) in YAML frontmatter, in the same forms as
/// [`frontmatter_tags`].
pub fn frontmatter_list(fm: &str, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_list = false;
    let clean = |t: &str| t.trim().trim_matches(['"', '\'']).to_string();
    for line in fm.lines() {
        if in_list {
            if let Some(item) = line.trim_start().strip_prefix("- ") {
                let t = clean(item);
                if !t.is_empty() {
                    out.push(t);
                }
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            in_list = false;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        if !keys.contains(&key.trim()) || line.starts_with([' ', '\t']) {
            continue;
        }
        let value = value.trim().trim_start_matches('[').trim_end_matches(']');
        if value.is_empty() {
            in_list = true;
            continue;
        }
        out.extend(value.split([',', ' ']).map(clean).filter(|t| !t.is_empty()));
    }
    out
}

/// The note's `cssclasses` (and the older `cssclass`), from its frontmatter.
pub fn cssclasses(text: &str) -> Vec<String> {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(body) = t.strip_prefix("---\n").or_else(|| t.strip_prefix("---\r\n")) else { return Vec::new() };
    let end = body.find("\n---").map(|i| i + 1).unwrap_or(0);
    if end == 0 && !body.starts_with("---") {
        return Vec::new();
    }
    frontmatter_list(&body[..end], &["cssclasses", "cssclass"])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backlink {
    pub source: String,
    /// Index into the source note's `links`.
    pub link: usize,
}

/// Edits to one note's text (byte ranges in its current text, non-overlapping, ascending).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteEdit {
    pub note: String,
    pub edits: Vec<(Range<usize>, String)>,
}

pub fn is_note(rel: &str) -> bool {
    rel.to_lowercase().ends_with(".md")
}

pub fn is_sync_conflict(rel: &str) -> bool {
    rel.contains(".sync-conflict-")
}

fn hidden(rel: &str) -> bool {
    rel.split('/').any(|c| c.starts_with('.'))
}

#[derive(Clone, Debug, Default)]
pub struct VaultIndex {
    pub root: PathBuf,
    files: BTreeSet<String>,
    resolver: Resolver,
    notes: HashMap<String, NoteMeta>,
    backlinks: HashMap<String, Vec<Backlink>>,
    /// Note texts for full-text search (kept folded, see `search::fold`).
    docs: HashMap<String, SearchDoc>,
    /// Bumped on every change, so views can cache what they derive from the index.
    generation: u64,
}

impl VaultIndex {
    /// Scans the vault folder and indexes every note (dot-folders such as `.obsidian` skipped).
    pub fn build(root: &Path) -> Self {
        let mut files = Vec::new();
        fn rec(root: &Path, d: &Path, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(d) else { return };
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    rec(root, &p, out);
                } else if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        rec(root, root, &mut files);
        let mut idx = Self { root: root.to_path_buf(), ..Default::default() };
        for f in &files {
            idx.files.insert(f.clone());
            idx.resolver.add(f);
        }
        for f in &files {
            if is_note(f) {
                if let Ok(text) = std::fs::read_to_string(root.join(f)) {
                    idx.put_note(f, &text);
                }
            }
        }
        idx.resolve_all();
        idx
    }

    /// Resolves a link from `from`; `[[#heading]]` / `[[#^id]]` (empty path) is the note itself.
    fn resolve_in(resolver: &Resolver, target: &str, from: &str) -> Option<String> {
        if link_path(target).is_none() {
            return target.trim_start().starts_with('#').then(|| from.to_string());
        }
        resolver.resolve(target, from)
    }

    /// Stores a note's metadata and search text; true if its metadata changed.
    fn put_note(&mut self, rel: &str, text: &str) -> bool {
        let meta = parse_note(rel, text);
        let changed = self.notes.get(rel) != Some(&meta);
        self.docs.insert(rel.to_string(), SearchDoc::new(rel, text, meta.tags.clone()));
        self.notes.insert(rel.to_string(), meta);
        self.bump();
        changed
    }

    fn drop_note(&mut self, rel: &str) {
        self.notes.remove(rel);
        self.docs.remove(rel);
        self.bump();
    }

    /// A new generation, unique in this process (a rebuilt index never repeats an old one,
    /// so caches keyed by it can't be fooled).
    fn bump(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.generation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Changes whenever anything in the index changes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Full-text search over every note (see [`crate::search`]).
    pub fn search(&self, q: &Expr) -> Vec<FileHit> {
        let docs: Vec<&SearchDoc> = self.docs.values().collect();
        search::search(q, &docs)
    }

    /// A note's text as last indexed (its saved content).
    pub fn text(&self, rel: &str) -> Option<&str> {
        self.docs.get(rel).map(|d| d.text.as_str())
    }

    /// Every tag in the vault (as first written, case-insensitively merged) with the number of
    /// occurrences, sorted by name.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut by_key: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for n in self.notes.values() {
            for t in &n.tags {
                let e = by_key.entry(search::fold(t)).or_insert_with(|| (t.clone(), 0));
                e.1 += 1;
            }
        }
        by_key.into_values().collect()
    }

    fn resolve_all(&mut self) {
        self.bump();
        let resolver = &self.resolver;
        self.backlinks.clear();
        for (path, n) in self.notes.iter_mut() {
            for (i, l) in n.links.iter_mut().enumerate() {
                l.resolved = Self::resolve_in(resolver, &l.target, path);
                if let Some(t) = &l.resolved {
                    self.backlinks.entry(t.clone()).or_default().push(Backlink { source: path.clone(), link: i });
                }
            }
        }
        for v in self.backlinks.values_mut() {
            v.sort_by(|a, b| a.source.cmp(&b.source).then(a.link.cmp(&b.link)));
        }
    }

    pub fn files(&self) -> impl Iterator<Item = &String> {
        self.files.iter()
    }
    pub fn contains(&self, rel: &str) -> bool {
        self.files.contains(rel)
    }
    pub fn note(&self, rel: &str) -> Option<&NoteMeta> {
        self.notes.get(rel)
    }
    pub fn notes(&self) -> impl Iterator<Item = &NoteMeta> {
        self.notes.values()
    }
    /// Shortest link text resolving to `target` (vault path) from the note `from`.
    pub fn shortest_link(&self, target: &str, from: &str) -> String {
        self.resolver.shortest_link(target, from, !is_note(target))
    }

    pub fn resolve(&self, link: &str, from: &str) -> Option<String> {
        Self::resolve_in(&self.resolver, link, from)
    }
    pub fn backlinks(&self, rel: &str) -> &[Backlink] {
        self.backlinks.get(rel).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn relative(&self, p: &Path) -> Option<String> {
        p.strip_prefix(&self.root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
    }

    /// A note was created or changed (text = its current content).
    pub fn upsert_note(&mut self, rel: &str, text: &str) {
        let new_file = self.files.insert(rel.to_string());
        if new_file {
            self.resolver.add(rel);
        }
        self.put_note(rel, text);
        // a new file can change how other links resolve; otherwise only this note's links changed
        self.resolve_all();
    }

    /// A non-note file appeared (image, pdf…).
    pub fn add_file(&mut self, rel: &str) {
        if self.files.insert(rel.to_string()) {
            self.resolver.add(rel);
            self.resolve_all();
        }
    }

    pub fn remove_file(&mut self, rel: &str) {
        if self.files.remove(rel) {
            self.resolver.remove(rel);
            self.drop_note(rel);
            self.resolve_all();
        }
    }

    /// Applies a watcher event for `rel`: re-reads the file from disk, or removes it if gone.
    pub fn refresh(&mut self, rel: &str) {
        self.refresh_batch(&[rel.to_string()]);
    }

    /// Applies a batch of watcher events with a single link-resolution pass at the end.
    /// A folder path indexes only that folder's contents (a folder moved in), never the vault.
    pub fn refresh_batch(&mut self, rels: &[String]) {
        let mut touched = false;
        for rel in rels {
            if hidden(rel) {
                continue;
            }
            let p = self.root.join(rel);
            if p.is_file() {
                touched |= self.load_file(rel);
            } else if p.is_dir() {
                let mut inside = Vec::new();
                fn rec(root: &Path, d: &Path, out: &mut Vec<String>) {
                    let Ok(rd) = std::fs::read_dir(d) else { return };
                    for e in rd.flatten() {
                        if e.file_name().to_string_lossy().starts_with('.') {
                            continue;
                        }
                        let p = e.path();
                        if p.is_dir() {
                            rec(root, &p, out);
                        } else if let Ok(r) = p.strip_prefix(root) {
                            out.push(r.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
                rec(&self.root, &p, &mut inside);
                for f in inside {
                    if !self.files.contains(&f) {
                        touched |= self.load_file(&f);
                    }
                }
            } else {
                let prefix = format!("{rel}/");
                let gone: Vec<String> = self.files.iter().filter(|f| *f == rel || f.starts_with(&prefix)).cloned().collect();
                for f in gone {
                    self.files.remove(&f);
                    self.resolver.remove(&f);
                    self.drop_note(&f);
                    touched = true;
                }
            }
        }
        if touched {
            self.resolve_all();
        }
    }

    /// (Re)reads one existing file into the index; no resolution pass. Returns whether anything changed.
    fn load_file(&mut self, rel: &str) -> bool {
        if self.files.insert(rel.to_string()) {
            self.resolver.add(rel);
        }
        if is_note(rel) {
            match std::fs::read_to_string(self.root.join(rel)) {
                Ok(text) => self.put_note(rel, &text),
                Err(_) => false,
            }
        } else {
            true
        }
    }

    /// Link edits needed when files move (`moves`: old -> new vault paths, files not folders).
    /// Every link whose target would change is rewritten to the shortest text that resolves to
    /// the moved target from its note's new location; `#subpath` and `|alias` are kept.
    /// Notes are identified by their path *after* the move.
    pub fn rename_plan(&self, moves: &[(String, String)]) -> Vec<NoteEdit> {
        self.link_plan(moves, false)
    }

    /// Link edits for merging note `src` into `dst` (note composer): every link to `src` is
    /// made to point at `dst`, keeping headings, block ids and aliases. Edits are reported by
    /// each note's current path (`src`'s own edits under `src`).
    pub fn merge_plan(&self, src: &str, dst: &str) -> Vec<NoteEdit> {
        self.link_plan(&[(src.to_string(), dst.to_string())], true)
    }

    fn link_plan(&self, moves: &[(String, String)], by_current_path: bool) -> Vec<NoteEdit> {
        let map = |p: &str| moves.iter().find(|(o, _)| o == p).map(|(_, n)| n.clone()).unwrap_or_else(|| p.to_string());
        let mut after = self.resolver.clone();
        for (o, n) in moves {
            after.remove(o);
            after.add(n);
        }
        let mut out = Vec::new();
        let mut paths: Vec<&String> = self.notes.keys().collect();
        paths.sort();
        for path in paths {
            let note = &self.notes[path];
            let new_path = map(path);
            let mut edits = Vec::new();
            for l in &note.links {
                let Some(old_target) = &l.resolved else { continue };
                let want = map(old_target);
                // still points at the (possibly moved) target: leave the text alone
                if Self::resolve_in(&after, &l.target, &new_path).as_deref() == Some(want.as_str()) {
                    continue;
                }
                let old_path_text = link_path(&l.target).unwrap_or("");
                let keep_ext = old_path_text.rsplit('/').next().is_some_and(|last| last.contains('.')) || !is_note(&want);
                let text = if l.markdown {
                    want.replace(' ', "%20")
                } else {
                    after.shortest_link(&want, &new_path, keep_ext)
                };
                edits.push((l.path_range.clone(), text));
            }
            if !edits.is_empty() {
                out.push(NoteEdit { note: if by_current_path { path.clone() } else { new_path }, edits });
            }
        }
        out
    }
}

/// Applies non-overlapping ascending edits to a text.
pub fn apply_edits(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (r, t) in edits {
        out.push_str(&text[pos..r.start]);
        out.push_str(t);
        pos = r.end;
    }
    out.push_str(&text[pos..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(files: &[(&str, &str)]) -> (PathBuf, VaultIndex) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ol-index-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (p, t) in files {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        }
        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        let idx = VaultIndex::build(&root);
        (root, idx)
    }

    #[test]
    fn metadata_links_and_backlinks() {
        let (root, idx) = vault(&[
            ("A.md", "---\ncssclasses: Plain\n---\n# Title\nsee [[B#Sec|here]] and ![[img.png]] and [[Missing]] #tag\n> [!thm|x=1] Theorem\n> testo ^blk\n"),
            ("sub/B.md", "## Sec\n[[A]]"),
            ("Images/img.png", ""),
        ]);
        let a = idx.note("A.md").unwrap();
        assert_eq!(a.frontmatter.as_deref(), Some("cssclasses: Plain"));
        assert_eq!(a.headings[0].text, "Title");
        let r: Vec<_> = a.links.iter().map(|l| (l.target.as_str(), l.resolved.as_deref())).collect();
        assert_eq!(r, vec![("B#Sec", Some("sub/B.md")), ("img.png", Some("Images/img.png")), ("Missing", None)]);
        assert_eq!(a.tags, vec!["tag"]);
        assert_eq!(a.block_ids, vec![("blk".to_string(), 6)]);
        assert_eq!((a.callouts[0].kind.as_str(), a.callouts[0].meta.as_str(), a.callouts[0].title.as_str()), ("thm", "x=1", "Theorem"));
        assert_eq!(idx.backlinks("sub/B.md"), &[Backlink { source: "A.md".into(), link: 0 }]);
        assert_eq!(idx.backlinks("A.md")[0].source, "sub/B.md");
        assert_eq!(idx.resolve("#^blk", "A.md").as_deref(), Some("A.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rename_keeps_subpath_alias_and_uses_shortest_path() {
        let (root, idx) = vault(&[
            ("A.md", "[[B#Sec|here]] ![[img.png]] [[B]] [[B#^lem1]]"),
            ("sub/B.md", "x"),
            ("other/B2.md", ""),
            ("Images/img.png", ""),
        ]);
        let plan = idx.rename_plan(&[("sub/B.md".into(), "sub/New name.md".into())]);
        assert_eq!(plan.len(), 1);
        let a = std::fs::read_to_string(root.join("A.md")).unwrap();
        assert_eq!(apply_edits(&a, &plan[0].edits), "[[New name#Sec|here]] ![[img.png]] [[New name]] [[New name#^lem1]]", "references keep their block id");
        // a move that creates a name clash needs a longer path
        let (root2, idx2) = vault(&[("A.md", "[[B]]"), ("x/B.md", ""), ("y/C.md", "")]);
        let plan = idx2.rename_plan(&[("y/C.md".into(), "B.md".into()), ("x/B.md".into(), "x/B.md".into())]);
        // root-level B.md now wins "B" (exact path), so A's link to x/B.md must become "x/B"
        let a = std::fs::read_to_string(root2.join("A.md")).unwrap();
        assert_eq!(apply_edits(&a, &plan[0].edits), "[[x/B]]");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(root2);
    }

    #[test]
    fn image_rename_keeps_extension_and_unrelated_links_untouched() {
        let (root, idx) = vault(&[("A.md", "![[a.png]] [[C]]"), ("Images/a.png", ""), ("C.md", "")]);
        let plan = idx.rename_plan(&[("Images/a.png".into(), "Images/b.png".into())]);
        let a = std::fs::read_to_string(root.join("A.md")).unwrap();
        assert_eq!(apply_edits(&a, &plan[0].edits), "![[b.png]] [[C]]");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn frontmatter_tags_and_search() {
        assert_eq!(frontmatter_tags("tags: [work, \"#exam\"]\ncssclasses: x"), vec!["work", "exam"]);
        assert_eq!(frontmatter_tags("tags:\n  - a/b\n  - c\naliases: x"), vec!["a/b", "c"]);
        assert_eq!(frontmatter_tags("tag: a, b"), vec!["a", "b"]);
        assert!(frontmatter_tags("cssclasses: Plain").is_empty());
        let (root, mut idx) = vault(&[("A.md", "---\ntags: [course]\n---\nGraphs #Course and trees"), ("B.md", "graphs")]);
        assert_eq!(idx.tags(), vec![("course".to_string(), 2)]);
        let q = crate::search::parse_query("graphs tag:course").unwrap();
        assert_eq!(idx.search(&q).iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), vec!["A.md"]);
        let g = idx.generation();
        std::fs::write(root.join("B.md"), "graphs #course").unwrap();
        idx.refresh("B.md");
        assert!(idx.generation() > g);
        assert_eq!(idx.search(&q).len(), 2, "search sees the new text");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_updates() {
        let (root, mut idx) = vault(&[("A.md", "[[New]]")]);
        assert_eq!(idx.note("A.md").unwrap().links[0].resolved, None);
        std::fs::write(root.join("New.md"), "").unwrap();
        idx.refresh("New.md");
        assert_eq!(idx.note("A.md").unwrap().links[0].resolved.as_deref(), Some("New.md"));
        std::fs::remove_file(root.join("New.md")).unwrap();
        idx.refresh("New.md");
        assert_eq!(idx.note("A.md").unwrap().links[0].resolved, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn merge_points_links_at_the_target() {
        let dir = std::env::temp_dir().join(format!("ol-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("x")).unwrap();
        std::fs::write(dir.join("x/Src.md"), "# H\nsee [[#H]] and [[Src#H]]\n").unwrap();
        std::fs::write(dir.join("Dst.md"), "dst [[Src|alias]]\n").unwrap();
        std::fs::write(dir.join("Other.md"), "[[Src#^b1]] ![[Src]] [[Dst]]\n").unwrap();
        let ix = VaultIndex::build(&dir);
        let plan = ix.merge_plan("x/Src.md", "Dst.md");
        let get = |n: &str| plan.iter().find(|e| e.note == n).map(|e| apply_edits(ix.text(n).unwrap(), &e.edits));
        assert_eq!(get("x/Src.md").as_deref(), Some("# H\nsee [[#H]] and [[Dst#H]]\n"));
        assert_eq!(get("Dst.md").as_deref(), Some("dst [[Dst|alias]]\n"));
        assert_eq!(get("Other.md").as_deref(), Some("[[Dst#^b1]] ![[Dst]] [[Dst]]\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn css_classes_from_frontmatter() {
        assert_eq!(cssclasses("---\ncssclasses: Plain\n---\n# x"), ["Plain"]);
        assert_eq!(cssclasses("---\ntitle: a\ncssclasses: [wide, \"Plain\"]\n---\n"), ["wide", "Plain"]);
        assert_eq!(cssclasses("---\ncssclasses:\n  - a\n  - b\ntags: x\n---\n"), ["a", "b"]);
        assert_eq!(cssclasses("---\ncssclass: old\n---\n"), ["old"]);
        assert!(cssclasses("# no frontmatter\ncssclasses: x").is_empty());
        assert!(cssclasses("---\ncssclasses: x\n").is_empty(), "unclosed");
    }
}
