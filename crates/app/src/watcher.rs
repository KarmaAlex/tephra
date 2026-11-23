//! The vault-wide file watcher (HANDOFF §2.4): turns filesystem events into the vault-relative
//! paths that changed, without any window, so the workspace only decides what to reload.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::event::{EventKind, ModifyKind};
use notify::Watcher as _;

/// Whether an event is a real change. Access events (open/read/close) and metadata changes are
/// not: re-indexing reads files and listing folders opens them, so reacting to those would
/// re-trigger itself forever (this caused ~1 s UI stalls every second).
pub fn is_relevant(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(_) | EventKind::Other | EventKind::Any => false,
    }
}

/// The vault-relative path (with `/`) of a changed file, or `None` for paths the app ignores:
/// outside the vault, the vault itself, dot-folders (`.obsidian`, `.obsidilight`, `.stfolder`…)
/// and the app's own temporary save files.
pub fn vault_relative(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    if rel.is_empty() || rel.split('/').any(|c| c.starts_with('.')) || rel.contains(".obsidilight-tmp-") {
        return None;
    }
    Some(rel)
}

/// Watches a vault recursively. Changed paths queue up until [`VaultWatcher::drain`] collects
/// them, so a burst of events (a Syncthing pull, a save's write and rename) becomes one batch.
pub struct VaultWatcher {
    root: PathBuf,
    rx: mpsc::Receiver<PathBuf>,
    _watcher: notify::RecommendedWatcher,
}

impl VaultWatcher {
    pub fn new(root: &Path) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if !is_relevant(&ev.kind) {
                return;
            }
            if std::env::var_os("OBSIDILIGHT_TIMING").is_some() {
                eprintln!("EVENT {:?} {:?}", ev.kind, ev.paths.first().and_then(|p| p.file_name()));
            }
            for p in ev.paths {
                let _ = tx.send(p);
            }
        })?;
        w.watch(root, notify::RecursiveMode::Recursive)?;
        Ok(Self { root: root.to_path_buf(), rx, _watcher: w })
    }

    /// The vault-relative paths changed since the last call, each once, in the order first seen.
    pub fn drain(&self) -> Vec<String> {
        let mut changed: Vec<String> = Vec::new();
        while let Ok(p) = self.rx.try_recv() {
            if let Some(rel) = vault_relative(&self.root, &p) {
                if !changed.contains(&rel) {
                    changed.push(rel);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode};
    use obsidilight_core::index::VaultIndex;
    use std::time::{Duration, Instant};

    fn vault(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ol-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        std::fs::create_dir_all(root.join("Corsi/Calcolabilità")).unwrap();
        std::fs::write(root.join("A.md"), "# A\n[[B]]\n").unwrap();
        std::fs::write(root.join("Corsi/Calcolabilità/Borůvka.md"), "# Borůvka\n").unwrap();
        root
    }

    /// Collects batches until `done` holds for everything seen so far, or fails after 5 s.
    fn wait_for(w: &VaultWatcher, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        let start = Instant::now();
        let mut seen = Vec::new();
        loop {
            for r in w.drain() {
                if !seen.contains(&r) {
                    seen.push(r);
                }
            }
            if done(&seen) {
                return seen;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "watcher saw only {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Lets the events of anything done so far arrive, then returns them.
    fn settle(w: &VaultWatcher) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(300));
        w.drain()
    }

    #[test]
    fn only_real_changes_are_relevant() {
        assert!(is_relevant(&EventKind::Create(CreateKind::File)));
        assert!(is_relevant(&EventKind::Remove(RemoveKind::File)));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Data(DataChange::Content))));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Name(RenameMode::Both))));
        assert!(!is_relevant(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))));
        assert!(!is_relevant(&EventKind::Access(AccessKind::Read)));
        assert!(!is_relevant(&EventKind::Other));
        assert!(!is_relevant(&EventKind::Any));
    }

    #[test]
    fn paths_are_vault_relative_and_skip_hidden_and_temp_files() {
        let root = Path::new("/v/Test vault");
        assert_eq!(vault_relative(root, &root.join("Corsi/Borůvka.md")).as_deref(), Some("Corsi/Borůvka.md"));
        assert_eq!(vault_relative(root, &root.join("Images/Pasted image 20250101.png")).as_deref(), Some("Images/Pasted image 20250101.png"));
        assert_eq!(vault_relative(root, &root.join("A.sync-conflict-20251010-120000-ABC.md")).as_deref(), Some("A.sync-conflict-20251010-120000-ABC.md"));
        assert_eq!(vault_relative(root, root), None);
        assert_eq!(vault_relative(root, Path::new("/v/other/A.md")), None);
        assert_eq!(vault_relative(root, &root.join(".obsidian/app.json")), None);
        assert_eq!(vault_relative(root, &root.join(".obsidilight/plugins/x.json")), None);
        assert_eq!(vault_relative(root, &root.join("Corsi/.stversions/A.md")), None);
        assert_eq!(vault_relative(root, &root.join("Corsi/.obsidilight-tmp-123-A.md")), None);
    }

    #[test]
    fn reports_creates_edits_renames_and_deletes_once_each() {
        let root = vault("ops");
        let w = VaultWatcher::new(&root).unwrap();

        std::fs::write(root.join("Corsi/Calcolabilità/Nuova.md"), "nuova").unwrap();
        std::fs::write(root.join("A.md"), "# A\nmodificata\n").unwrap();
        std::fs::write(root.join("A.md"), "# A\nmodificata due volte\n").unwrap();
        let seen = wait_for(&w, |s| s.iter().any(|r| r == "A.md") && s.iter().any(|r| r == "Corsi/Calcolabilità/Nuova.md"));
        assert_eq!(seen.iter().filter(|r| *r == "A.md").count(), 1, "{seen:?}");

        std::fs::rename(root.join("Corsi/Calcolabilità/Borůvka.md"), root.join("Corsi/MST.md")).unwrap();
        wait_for(&w, |s| s.iter().any(|r| r == "Corsi/Calcolabilità/Borůvka.md") && s.iter().any(|r| r == "Corsi/MST.md"));

        std::fs::remove_file(root.join("Corsi/Calcolabilità/Nuova.md")).unwrap();
        wait_for(&w, |s| s.iter().any(|r| r == "Corsi/Calcolabilità/Nuova.md"));

        // A folder arriving with a file in it (Syncthing, a move into the vault): the file can
        // land before the new folder is watched, so the folder itself is reported, and
        // `refresh_batch` indexes what it contains.
        let mut idx = VaultIndex::build(&root);
        std::fs::create_dir_all(root.join("Nuovo corso")).unwrap();
        std::fs::write(root.join("Nuovo corso/Lezione 1.md"), "x").unwrap();
        let batch = wait_for(&w, |s| s.iter().any(|r| r == "Nuovo corso" || r == "Nuovo corso/Lezione 1.md"));
        idx.refresh_batch(&batch);
        assert!(idx.contains("Nuovo corso/Lezione 1.md"));
        // and the new folder is watched from then on
        settle(&w);
        std::fs::write(root.join("Nuovo corso/Lezione 2.md"), "y").unwrap();
        wait_for(&w, |s| s.iter().any(|r| r == "Nuovo corso/Lezione 2.md"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_reads_hidden_folders_and_own_temp_files() {
        let root = vault("quiet");
        let w = VaultWatcher::new(&root).unwrap();
        settle(&w);

        // reading and listing (what re-indexing does) must not re-trigger the watcher
        let _ = std::fs::read_to_string(root.join("A.md")).unwrap();
        let _ = std::fs::read_dir(root.join("Corsi")).unwrap().count();
        let _ = VaultIndex::build(&root);
        // Obsidian's and the app's config, Syncthing's folders, the app's atomic-save temp file
        std::fs::write(root.join(".obsidian/workspace.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join(".stfolder")).unwrap();
        std::fs::write(root.join("Corsi/.obsidilight-tmp-1-A.md"), "x").unwrap();
        assert_eq!(settle(&w), Vec::<String>::new());

        // still alive afterwards
        std::fs::write(root.join("A.md"), "dopo").unwrap();
        wait_for(&w, |s| s == ["A.md"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The workspace's path: watcher batches feed `refresh_batch`, which updates the index
    /// incrementally (an outside edit from Syncthing, a new note, a deleted one).
    #[test]
    fn batches_keep_the_index_current() {
        let root = vault("index");
        let mut idx = VaultIndex::build(&root);
        assert_eq!(idx.resolve("B", "A.md"), None);
        let w = VaultWatcher::new(&root).unwrap();

        std::fs::write(root.join("B.md"), "# B\n").unwrap();
        std::fs::write(root.join("A.md"), "# A\n[[B]] #nuovo\n").unwrap();
        std::fs::remove_file(root.join("Corsi/Calcolabilità/Borůvka.md")).unwrap();
        let batch = wait_for(&w, |s| s.len() >= 3);
        idx.refresh_batch(&batch);

        assert_eq!(idx.resolve("B", "A.md").as_deref(), Some("B.md"));
        assert_eq!(idx.text("A.md"), Some("# A\n[[B]] #nuovo\n"));
        assert_eq!(idx.backlinks("B.md").len(), 1);
        assert!(idx.tags().iter().any(|(t, _)| t == "nuovo"));
        assert!(!idx.contains("Corsi/Calcolabilità/Borůvka.md"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
