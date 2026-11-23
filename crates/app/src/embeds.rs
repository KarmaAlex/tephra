//! The open vault, shared by the workspace and every editor: the index (link resolution,
//! metadata) plus attachment handling (pasted/dropped images). `.obsidian/` is only read.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use obsidilight_core::index::VaultIndex;

pub type SharedVault = Rc<RefCell<Vault>>;

pub struct Vault {
    pub root: PathBuf,
    pub index: VaultIndex,
    /// From `.obsidian/app.json` `attachmentFolderPath` (vault-relative, "" = root).
    attachment_dir: String,
}

impl Vault {
    /// The vault containing `p` (a note or a folder): the nearest ancestor with `.obsidian/`,
    /// else the folder itself / the note's folder.
    pub fn root_for(p: &Path) -> PathBuf {
        let dir = if p.is_dir() { p.to_path_buf() } else { p.parent().unwrap_or(Path::new(".")).to_path_buf() };
        dir.ancestors().find(|a| a.join(".obsidian").is_dir()).map(Path::to_path_buf).unwrap_or(dir)
    }

    pub fn open(root: PathBuf) -> Self {
        let attachment_dir = std::fs::read_to_string(root.join(".obsidian/app.json"))
            .ok()
            .and_then(|s| json_string_field(&s, "attachmentFolderPath"))
            .map(|s| s.trim_start_matches("./").trim_end_matches('/').to_string())
            .unwrap_or_default();
        let index = VaultIndex::build(&root);
        Self { root, index, attachment_dir }
    }

    pub fn relative(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().replace('\\', "/")
    }

    /// Resolves a link target from the note at `from` (absolute path) to an absolute path.
    pub fn resolve(&self, target: &str, from: &Path) -> Option<PathBuf> {
        self.index.resolve(target, &self.relative(from)).map(|r| self.root.join(r))
    }

    fn attachments(&self) -> io::Result<PathBuf> {
        let dir = self.root.join(&self.attachment_dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn write_new(&mut self, dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write as _;
        // create_new: never overwrite an existing file
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(name))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        let rel = self.relative(&dir.join(name));
        self.index.add_file(&rel);
        Ok(())
    }

    /// Saves pasted image bytes as `<attachments>/Pasted image YYYYMMDDHHmmss.png`
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
        let dir = self.attachments()?;
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
        let mut name = format!("Pasted image {stamp}.png");
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("Pasted image {stamp} {n}.png");
            n += 1;
        }
        self.write_new(&dir, &name, &png)?;
        Ok(name)
    }

    /// Copies a dropped file into the attachment folder (keeping its name; ` 1`, ` 2`…
    /// appended if taken) and returns the name to embed. Never overwrites.
    pub fn import_file(&mut self, src: &Path) -> io::Result<String> {
        let dir = self.attachments()?;
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let ext = src.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("{stem} {n}.{ext}");
            n += 1;
        }
        let bytes = std::fs::read(src)?;
        self.write_new(&dir, &name, &bytes)?;
        Ok(name)
    }
}

/// The open vault's folder, for code that runs while the workspace is busy (a tab view being
/// created from inside the workspace's own update).
pub struct VaultRoot(pub PathBuf);
impl gpui_kit::Global for VaultRoot {}

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
