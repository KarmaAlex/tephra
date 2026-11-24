//! Moving files and folders by dragging them in the file list (M8), as pure path logic: which
//! folder a drop lands in, and where the dragged item ends up. The move itself is a rename
//! (`Workspace::rename_path`), so links are updated as on rename.

/// What is dragged: a vault path, and whether it is a folder.
#[derive(Clone, Debug)]
pub struct TreeDrag {
    pub rel: String,
    pub folder: bool,
}

/// Whether a file-list row is a folder: `rebuild_tree` gives every directory row the `dir:` id
/// prefix. (The tree's own `is_folder()` means "has children", so it misses empty folders.)
pub fn is_dir_row(row: &str) -> bool {
    row.starts_with("dir:")
}

/// The folder a drop on a file-list row moves into: the folder itself for a folder row
/// (`dir:<path>`), the folder holding the file for a file row ("" = the vault root).
pub fn dest_of_row(row: &str) -> String {
    match row.strip_prefix("dir:") {
        Some(d) => d.to_string(),
        None => parent(row).to_string(),
    }
}

pub fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Where `src` goes when dropped into folder `dest`, or None when that isn't a move: into the
/// folder it is already in, or a folder into itself or a folder inside it.
pub fn move_target(src: &str, folder: bool, dest: &str) -> Option<String> {
    if src.is_empty() || parent(src) == dest {
        return None;
    }
    if folder && (dest == src || dest.starts_with(&format!("{src}/"))) {
        return None;
    }
    let name = src.rsplit('/').next().unwrap_or(src);
    Some(if dest.is_empty() { name.to_string() } else { format!("{dest}/{name}") })
}

/// Whether row `row` is drawn as part of the drop folder `dest` (the folder's row and
/// everything shown inside it).
pub fn in_dest(row: &str, dest: &str) -> bool {
    let path = row.strip_prefix("dir:").unwrap_or(row);
    !dest.is_empty() && (path == dest && row.starts_with("dir:") || path.starts_with(&format!("{dest}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_folders() {
        assert_eq!(dest_of_row("dir:Corsi/Algoritmi"), "Corsi/Algoritmi");
        assert_eq!(dest_of_row("Corsi/Algoritmi/Borůvka.md"), "Corsi/Algoritmi");
        assert_eq!(dest_of_row("Note.md"), "");
    }

    #[test]
    fn targets() {
        // a note into a folder, an attachment to the root, Unicode names
        assert_eq!(move_target("Altro/Calcolabilità.md", false, "Corsi").as_deref(), Some("Corsi/Calcolabilità.md"));
        assert_eq!(move_target("Images/Pasted image 20250101.png", false, "").as_deref(), Some("Pasted image 20250101.png"));
        // a folder with everything in it
        assert_eq!(move_target("Corsi/Algoritmi", true, "Archivio").as_deref(), Some("Archivio/Algoritmi"));
        // not moves
        assert_eq!(move_target("Corsi/a.md", false, "Corsi"), None, "same folder");
        assert_eq!(move_target("a.md", false, ""), None, "already at the root");
        assert_eq!(move_target("Corsi", true, "Corsi"), None, "into itself");
        assert_eq!(move_target("Corsi", true, "Corsi/Algoritmi"), None, "into a folder inside it");
        // a sibling whose name starts the same is fine
        assert_eq!(move_target("Corsi", true, "Corsi 2").as_deref(), Some("Corsi 2/Corsi"));
    }

    #[test]
    fn highlighted_rows() {
        assert!(in_dest("dir:Corsi", "Corsi"));
        assert!(in_dest("Corsi/a.md", "Corsi"));
        assert!(in_dest("dir:Corsi/Algoritmi", "Corsi"));
        assert!(!in_dest("dir:Corsi 2", "Corsi"));
        assert!(!in_dest("Corsi.md", "Corsi"));
        assert!(!in_dest("a.md", ""), "the root is drawn as the whole list");
    }
}
