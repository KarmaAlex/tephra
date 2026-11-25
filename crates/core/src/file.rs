//! Loading and saving notes without ever clobbering a newer file on disk.
//!
//! The rule: a save only goes through if the bytes currently on disk are exactly the
//! bytes we loaded (or last saved). Otherwise the file changed underneath us (Syncthing,
//! another editor) and the caller gets [`SaveOutcome::Conflict`] instead.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::Path;

/// Fingerprint of a file's content as we last saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskState {
    pub len: u64,
    pub hash: u64,
}

impl DiskState {
    pub fn of(bytes: &[u8]) -> Self {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        Self { len: bytes.len() as u64, hash: h.finish() }
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub text: String,
    pub disk: DiskState,
}

/// Reads a note. Non-UTF-8 files are an `InvalidData` error (the caller opens them read-only).
pub fn load(path: &Path) -> io::Result<Loaded> {
    let bytes = fs::read(path)?;
    let disk = DiskState::of(&bytes);
    let text = String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Loaded { text, disk })
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Written; the new fingerprint to compare against next time.
    Saved(DiskState),
    /// Nothing changed on disk and the text is identical: no write was needed.
    Unchanged(DiskState),
    /// The file on disk is not what we loaded. Nothing was written.
    Conflict { on_disk: Option<DiskState> },
}

/// Atomically writes `text` to `path` if the disk still matches `expected`.
/// `expected = None` means "the file must not exist yet" (new note).
pub fn save(path: &Path, text: &str, expected: Option<DiskState>) -> io::Result<SaveOutcome> {
    let current = match fs::read(path) {
        Ok(b) => Some(DiskState::of(&b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if current != expected {
        return Ok(SaveOutcome::Conflict { on_disk: current });
    }
    let new = DiskState::of(text.as_bytes());
    if current == Some(new) {
        return Ok(SaveOutcome::Unchanged(new));
    }
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let tmp = dir.join(format!(".{}.tephra-tmp-{}", name.to_string_lossy(), std::process::id()));
    let res = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)
    })();
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(SaveOutcome::Saved(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tephra-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_roundtrip_and_conflict() {
        let d = tmpdir("save");
        let p = d.join("Nota è.md");
        fs::write(&p, "ciao\r\n").unwrap();
        let l = load(&p).unwrap();
        assert_eq!(l.text, "ciao\r\n");
        // unchanged text: no write
        assert_eq!(save(&p, &l.text, Some(l.disk)).unwrap(), SaveOutcome::Unchanged(l.disk));
        // normal save
        let SaveOutcome::Saved(s1) = save(&p, "ciao mondo\r\n", Some(l.disk)).unwrap() else { panic!() };
        assert_eq!(fs::read_to_string(&p).unwrap(), "ciao mondo\r\n");
        // someone else writes: our next save must not clobber it
        fs::write(&p, "from phone").unwrap();
        let r = save(&p, "mine", Some(s1)).unwrap();
        assert!(matches!(r, SaveOutcome::Conflict { on_disk: Some(_) }));
        assert_eq!(fs::read_to_string(&p).unwrap(), "from phone");
        // new file must not exist yet
        let q = d.join("new.md");
        assert!(matches!(save(&q, "x", None).unwrap(), SaveOutcome::Saved(_)));
        assert!(matches!(save(&q, "y", None).unwrap(), SaveOutcome::Conflict { .. }));
        // no temp files left behind
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn non_utf8_is_invalid_data() {
        let d = tmpdir("utf8");
        let p = d.join("bin.md");
        fs::write(&p, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(load(&p).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&d);
    }
}
