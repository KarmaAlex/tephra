//! The open vault's files (for embed resolution) and pasted-image saving.
//! `.obsidian/` is only ever read.

use std::io;
use std::path::{Path, PathBuf};

use obsidilight_core::links::resolve_link;

pub struct Vault {
    pub root: PathBuf,
    files: Vec<String>,
    /// From `.obsidian/app.json` `attachmentFolderPath` (vault-relative, "" = root).
    attachment_dir: String,
}

impl Vault {
    /// The vault containing `note`: the nearest ancestor with a `.obsidian` folder, else the note's folder.
    pub fn for_note(note: &Path) -> Self {
        let dir = note.parent().unwrap_or(Path::new(".")).to_path_buf();
        let root = dir.ancestors().find(|a| a.join(".obsidian").is_dir()).map(Path::to_path_buf).unwrap_or(dir);
        Self::open(root)
    }

    pub fn open(root: PathBuf) -> Self {
        let attachment_dir = std::fs::read_to_string(root.join(".obsidian/app.json"))
            .ok()
            .and_then(|s| json_string_field(&s, "attachmentFolderPath"))
            .map(|s| s.trim_start_matches("./").trim_end_matches('/').to_string())
            .unwrap_or_default();
        let mut v = Self { root, files: Vec::new(), attachment_dir };
        v.rescan();
        v
    }

    pub fn rescan(&mut self) {
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
        self.files.clear();
        let root = self.root.clone();
        rec(&root, &root, &mut self.files);
        self.files.sort();
    }

    pub fn relative(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().replace('\\', "/")
    }

    /// Resolves an embed/link target from the note at `from` (absolute path).
    pub fn resolve(&self, target: &str, from: &Path) -> Option<PathBuf> {
        let refs: Vec<&str> = self.files.iter().map(String::as_str).collect();
        resolve_link(target, &refs, &self.relative(from)).map(|r| self.root.join(r))
    }

    /// Saves pasted/dropped image bytes as `<attachments>/Pasted image YYYYMMDDHHmmss.png`
    /// (Obsidian's naming) and returns the file name to embed.
    pub fn save_pasted_image(&mut self, bytes: &[u8], is_png: bool) -> io::Result<String> {
        let png = if is_png {
            bytes.to_vec()
        } else {
            let img = image::load_from_memory(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let mut out = Vec::new();
            img.write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(io::Error::other)?;
            out
        };
        let dir = self.root.join(&self.attachment_dir);
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
        let mut name = format!("Pasted image {stamp}.png");
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("Pasted image {stamp} {n}.png");
            n += 1;
        }
        // create_new: never overwrite an existing file
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name))?;
        f.write_all(&png)?;
        f.sync_all()?;
        let rel = self.relative(&dir.join(&name));
        self.files.push(rel);
        self.files.sort();
        Ok(name)
    }
}

impl Vault {
    /// Copies a dropped image file into the attachment folder (keeping its name; ` 1`, ` 2`…
    /// appended if taken) and returns the name to embed. Never overwrites.
    pub fn import_file(&mut self, src: &Path) -> io::Result<String> {
        let dir = self.root.join(&self.attachment_dir);
        std::fs::create_dir_all(&dir)?;
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let ext = src.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("{stem} {n}.{ext}");
            n += 1;
        }
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name))?;
        io::copy(&mut std::fs::File::open(src)?, &mut out)?;
        out.sync_all()?;
        let rel = self.relative(&dir.join(&name));
        self.files.push(rel);
        self.files.sort();
        Ok(name)
    }
}

pub fn is_image(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"))
}

/// Minimal `"key": "value"` lookup in a flat JSON object (no escapes inside the value).
fn json_string_field(json: &str, key: &str) -> Option<String> {
    let k = json.find(&format!("\"{key}\""))?;
    let rest = &json[k + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// Natural pixel size of an image file, read from its header only.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read as _;
    let mut head = [0u8; 24];
    let mut f = std::fs::File::open(path).ok()?;
    if f.read_exact(&mut head).is_ok() && head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((u32::from_be_bytes(head[16..20].try_into().ok()?), u32::from_be_bytes(head[20..24].try_into().ok()?)));
    }
    image::image_dimensions(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_json_attachment_folder() {
        let j = "{\n  \"attachmentFolderPath\": \"Images\",\n  \"x\": 1\n}";
        assert_eq!(json_string_field(j, "attachmentFolderPath").as_deref(), Some("Images"));
    }

    #[test]
    fn paste_saves_with_obsidian_name_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!("ol-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        std::fs::write(root.join(".obsidian/app.json"), r#"{"attachmentFolderPath": "Images"}"#).unwrap();
        let mut v = Vault::open(root.clone());
        let png = {
            let img = image::RgbaImage::new(2, 3);
            let mut out = Vec::new();
            image::DynamicImage::ImageRgba8(img).write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
            out
        };
        let a = v.save_pasted_image(&png, true).unwrap();
        let b = v.save_pasted_image(&png, true).unwrap();
        assert!(a.starts_with("Pasted image ") && a.ends_with(".png"));
        assert_ne!(a, b);
        assert_eq!(image_size(&root.join("Images").join(&a)), Some((2, 3)));
        assert!(v.resolve(&a, &root.join("note.md")).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
