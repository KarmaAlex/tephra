//! Vaults the app knows about, kept outside any vault: the last one opened (reopened when the
//! app starts without an argument) and the recently opened ones (the welcome page lists them).
//! Also the three ways to get a vault: open a folder, open a folder as a vault, create a vault.
//!
//! Saved in `$TEPHRA_CONFIG_DIR/vaults.json`; else `$XDG_CONFIG_HOME/tephra/vaults.json`
//! (`~/.config/tephra`), on Windows `%APPDATA%\tephra`. Scripted UI tests and the
//! benchmark never touch the real file: without `$TEPHRA_CONFIG_DIR` they save nothing. The same
//! folder ([`config_dir`]) holds the themes installed for every vault (`themes/`, see
//! `theme::themes_dir`).
//!
//! The app holds one vault per process (its settings, plugins and theme are globals), so a
//! vault opened from a window that already has one opens in a new window (a new process), as
//! Obsidian opens another vault in a new window.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, WindowExt as _};
use gpui_kit::*;
use serde_json::{json, Value};

use crate::embeds::{Vault, VaultRoot};

/// How many recent vaults are remembered.
const MAX_RECENT: usize = 20;

/// The saved list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Known {
    /// The vault opened last (in any window).
    pub last: Option<PathBuf>,
    /// Recently opened vaults, most recent first.
    pub recent: Vec<PathBuf>,
}

/// The app's per-user folder, next to no vault: `$TEPHRA_CONFIG_DIR`; else
/// `$XDG_CONFIG_HOME/tephra` (`~/.config/tephra`), on Windows `%APPDATA%\tephra`. Holds the vault
/// list (`vaults.json`) and the themes that aren't bundled with the app (`themes/`).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TEPHRA_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    let env_dir = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_absolute());
    // Windows: %APPDATA%\tephra (roamed, like other per-user settings)
    let base = if cfg!(windows) { env_dir("APPDATA") } else { None }
        .or_else(|| env_dir("XDG_CONFIG_HOME"))
        .or_else(|| env_dir("HOME").or_else(|| env_dir("USERPROFILE")).map(|h| h.join(".config")))?;
    Some(base.join("tephra"))
}

/// Where the list is saved (None: nowhere, in scripted runs without `$TEPHRA_CONFIG_DIR`).
pub fn state_file() -> Option<PathBuf> {
    if std::env::var_os("TEPHRA_CONFIG_DIR").is_none() && (std::env::var_os("TEPHRA_SCRIPT").is_some() || std::env::var_os("TEPHRA_BENCH").is_some()) {
        return None;
    }
    config_dir().map(|d| d.join("vaults.json"))
}

impl Known {
    pub fn parse(text: &str) -> Self {
        let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let path = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(PathBuf::from);
        Known {
            last: v.get("last").and_then(path),
            recent: v.get("recent").and_then(Value::as_array).map(|a| a.iter().filter_map(path).collect()).unwrap_or_default(),
        }
    }

    pub fn to_json(&self) -> String {
        let s = |p: &PathBuf| p.to_string_lossy().to_string();
        serde_json::to_string_pretty(&json!({ "last": self.last.as_ref().map(s), "recent": self.recent.iter().map(s).collect::<Vec<_>>() })).unwrap_or_default()
    }

    pub fn read(file: &Path) -> Self {
        std::fs::read_to_string(file).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Written atomically (another window may read it at the same time).
    pub fn write(&self, file: &Path) -> std::io::Result<()> {
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = file.with_extension("json.tephra-tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, file)
    }

    pub fn load() -> Self {
        state_file().map(|f| Self::read(&f)).unwrap_or_default()
    }

    /// `root` was opened: it becomes the last vault and the first recent one.
    pub fn opened(&mut self, root: &Path) {
        self.recent.retain(|p| p != root);
        self.recent.insert(0, root.to_path_buf());
        self.recent.truncate(MAX_RECENT);
        self.last = Some(root.to_path_buf());
    }

    /// Removes `root` from the list (the welcome page's ×).
    pub fn forget(&mut self, root: &Path) {
        self.recent.retain(|p| p != root);
        if self.last.as_deref() == Some(root) {
            self.last = None;
        }
    }

    /// The vault to open when the app starts without an argument (if it still exists).
    pub fn startup_vault(&self) -> Option<PathBuf> {
        self.last.clone().filter(|p| p.is_dir())
    }
}

/// Re-reads, changes and saves the list (other windows may have changed it meanwhile).
fn update(f: impl FnOnce(&mut Known)) {
    let Some(file) = state_file() else { return };
    let mut k = Known::read(&file);
    f(&mut k);
    if let Err(e) = k.write(&file) {
        eprintln!("could not save {}: {e}", file.display());
    }
}

pub fn record_opened(root: &Path) {
    update(|k| k.opened(root));
}

pub fn forget(root: &Path) {
    update(|k| k.forget(root));
}

/// Characters a vault name can't contain (folder names on Linux, Windows and Android).
const BAD_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Expands a leading `~` (home folder) in a typed location.
fn expand_home(s: &str) -> PathBuf {
    let home = || std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => match home() {
            Some(h) => h.join(rest.trim_start_matches(['/', '\\'])),
            None => PathBuf::from(s),
        },
        _ => PathBuf::from(s),
    }
}

/// The folder a new vault called `name` gets under `location`, or why it can't be created.
pub fn new_vault_path(location: &str, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for the vault.".into());
    }
    if name == "." || name == ".." || name.starts_with('.') || name.contains(BAD_CHARS) {
        return Err(format!("A vault name can't start with a dot or contain any of {}", BAD_CHARS.iter().collect::<String>()));
    }
    let location = location.trim();
    if location.is_empty() {
        return Err("Choose where to create the vault.".into());
    }
    let loc = expand_home(location);
    if !loc.is_absolute() {
        return Err(format!("\"{location}\" is not a full path."));
    }
    if !loc.is_dir() {
        return Err(format!("The folder \"{}\" does not exist.", loc.display()));
    }
    let dir = loc.join(name);
    if dir.exists() {
        return Err(format!("\"{}\" already exists.", dir.display()));
    }
    Ok(dir)
}

/// Creates the vault folder and its (empty) `.obsidian/`, which marks it as a vault for both
/// this app and Obsidian.
pub fn create_vault(location: &str, name: &str) -> Result<PathBuf, String> {
    let dir = new_vault_path(location, name)?;
    std::fs::create_dir(&dir).map_err(|e| format!("Could not create \"{}\": {e}", dir.display()))?;
    mark_as_vault(&dir).map_err(|e| format!("Could not create \"{}\": {e}", dir.join(".obsidian").display()))?;
    Ok(dir)
}

/// Adds an empty `.obsidian/` if the folder has none (an existing one is never touched).
pub fn mark_as_vault(dir: &Path) -> std::io::Result<()> {
    let marker = dir.join(".obsidian");
    if marker.is_dir() {
        return Ok(());
    }
    std::fs::create_dir(marker)
}

/// Where "Create new vault" proposes to put it: next to the last vault, else Documents, else home.
fn default_location() -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let docs = home.as_ref().map(|h| h.join("Documents")).filter(|d| d.is_dir());
    Known::load()
        .last
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .filter(|p| p.is_dir())
        .or(docs)
        .or(home)
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// How a chosen folder is opened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenAs {
    /// As it is: a folder inside a vault opens that vault, any other folder opens on its own.
    Folder,
    /// The folder itself becomes a vault (gets `.obsidian/` if it has none).
    Vault,
}

/// Opens `path`: in this process if it has no vault yet (the welcome page, whose window then
/// closes), else in a new window.
pub fn open(path: PathBuf, how: OpenAs, window: &mut Window, cx: &mut App) {
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    if !path.is_dir() {
        window.push_notification(format!("\"{}\" is not a folder.", path.display()), cx);
        return;
    }
    if how == OpenAs::Vault {
        if let Err(e) = mark_as_vault(&path) {
            window.push_notification(format!("Could not make \"{}\" a vault: {e}", path.display()), cx);
            return;
        }
    }
    let root = Vault::root_for(&path);
    match cx.try_global::<VaultRoot>().map(|r| r.0.clone()) {
        Some(current) if current == root => {
            let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            window.push_notification(format!("\"{name}\" is already open in this window."), cx);
        }
        Some(_) => match open_in_new_window(&root) {
            Ok(()) => {
                // a vault manager window opened from a vault has done its job
                if crate::welcome::is_welcome(window, cx) {
                    window.remove_window();
                }
            }
            Err(e) => window.push_notification(format!("Could not open a new window: {e}"), cx),
        },
        None => {
            crate::open_vault(root, Instant::now(), cx);
            window.remove_window();
        }
    }
}

/// Starts another copy of the app on `root`. Scripted UI tests only print what they'd open.
fn open_in_new_window(root: &Path) -> std::io::Result<()> {
    if std::env::var_os("TEPHRA_SCRIPT").is_some() {
        println!("NEW-WINDOW {}", root.display());
        return Ok(());
    }
    let mut child = std::process::Command::new(std::env::current_exe()?).arg(root).env_remove("TEPHRA_BENCH").spawn()?;
    // reap it when it exits (no zombie while this window stays open)
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Asks for a folder (the system's folder picker), then opens it.
pub fn pick_and_open(how: OpenAs, window: &mut Window, cx: &mut App) {
    let prompt = match how {
        OpenAs::Folder => "Open folder",
        OpenAs::Vault => "Open folder as vault",
    };
    let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(prompt.into()) });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = picked.await;
        let _ = handle.update(cx, |_, window, cx| match result {
            Ok(Ok(Some(paths))) => {
                if let Some(p) = paths.into_iter().next() {
                    open(p, how, window, cx);
                }
            }
            Ok(Err(e)) => window.push_notification(format!("Could not show the folder picker: {e}"), cx),
            _ => {}
        });
    })
    .detach();
}

/// "Create new vault": a name and a location (typed, or chosen with Browse…).
pub fn create_vault_dialog(window: &mut Window, cx: &mut App) {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("Vault name"));
    let location = cx.new(|cx| InputState::new(window, cx).default_value(default_location()));
    let focus = name.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let (name, location) = (name.clone(), location.clone());
        let muted = cx.theme().muted_foreground;
        let label = |s: &'static str| div().text_sm().text_color(muted).child(s);
        let browse = {
            let location = location.clone();
            Button::new("vault-browse").label("Browse…").outline().on_click(move |_, window, cx| {
                let picked = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose where to create the vault".into()) });
                let (handle, location) = (window.window_handle(), location.clone());
                cx.spawn(async move |cx| {
                    if let Ok(Ok(Some(paths))) = picked.await {
                        if let Some(p) = paths.into_iter().next() {
                            let _ = handle.update(cx, |_, window, cx| location.update(cx, |s, cx| s.set_value(p.display().to_string(), window, cx)));
                        }
                    }
                })
                .detach();
            })
        };
        dialog
            .title("Create new vault")
            .child(
                v_flex()
                    .gap_2()
                    .child(label("Name"))
                    .child(Input::new(&name))
                    .child(label("Location"))
                    .child(h_flex().gap_2().child(div().flex_1().min_w_0().child(Input::new(&location))).child(browse)),
            )
            .footer(
                DialogFooter::new()
                    .child(DialogClose::new().child(Button::new("cancel").label("Cancel").outline()))
                    .child(DialogAction::new().child(Button::new("ok").label("Create").primary())),
            )
            .on_ok(move |_, window, cx| {
                let (n, l) = (name.read(cx).value().to_string(), location.read(cx).value().to_string());
                match create_vault(&l, &n) {
                    Ok(dir) => {
                        // after this dialog closes (opening may close its window)
                        window.defer(cx, move |window, cx| open(dir, OpenAs::Vault, window, cx));
                        true
                    }
                    Err(e) => {
                        window.push_notification(e, cx);
                        false
                    }
                }
            })
    });
    window.focus(&focus, cx);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{create_vault, expand_home, mark_as_vault, new_vault_path, Known, Vault, MAX_RECENT};

    #[test]
    fn opened_moves_to_front_and_becomes_last() {
        let mut k = Known::default();
        k.opened(Path::new("/a"));
        k.opened(Path::new("/b"));
        k.opened(Path::new("/a"));
        assert_eq!(k.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(k.last.as_deref(), Some(Path::new("/a")));
        for i in 0..2 * MAX_RECENT {
            k.opened(&PathBuf::from(format!("/v{i}")));
        }
        assert_eq!(k.recent.len(), MAX_RECENT);
    }

    #[test]
    fn forget_clears_last() {
        let mut k = Known::default();
        k.opened(Path::new("/a"));
        k.opened(Path::new("/b"));
        k.forget(Path::new("/b"));
        assert_eq!(k.recent, vec![PathBuf::from("/a")]);
        assert_eq!(k.last, None);
    }

    #[test]
    fn json_round_trip_and_bad_input() {
        let mut k = Known::default();
        k.opened(Path::new("/x/Test vault"));
        k.opened(Path::new("/y/calcolabilità"));
        assert_eq!(Known::parse(&k.to_json()), k);
        assert_eq!(Known::parse("not json"), Known::default());
        assert_eq!(Known::parse(r#"{"last": 3, "recent": ["", "/a", 1]}"#), Known { last: None, recent: vec![PathBuf::from("/a")] });
    }

    #[test]
    fn startup_vault_must_exist() {
        let dir = std::env::temp_dir().join(format!("tephra-vaults-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut k = Known::default();
        k.opened(&dir);
        assert_eq!(k.startup_vault(), Some(dir.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(k.startup_vault(), None);
    }

    #[test]
    fn creating_a_vault() {
        let loc = std::env::temp_dir().join(format!("tephra-create-{}", std::process::id()));
        std::fs::create_dir_all(&loc).unwrap();
        let l = loc.to_string_lossy().to_string();
        for bad in ["", "  ", ".hidden", "..", "a/b", "a\\b", "what?"] {
            assert!(new_vault_path(&l, bad).is_err(), "{bad:?} accepted");
        }
        assert!(new_vault_path("", "ok").is_err());
        assert!(new_vault_path("relative/dir", "ok").is_err());
        assert!(new_vault_path(&loc.join("missing").to_string_lossy(), "ok").is_err());
        let dir = create_vault(&l, " Borůvka ").unwrap();
        assert_eq!(dir, loc.join("Borůvka"));
        assert!(dir.join(".obsidian").is_dir());
        assert_eq!(Vault::root_for(&dir), dir);
        assert!(create_vault(&l, "Borůvka").is_err(), "an existing folder is never reused");
        // marking keeps an existing .obsidian/ as it is
        std::fs::write(dir.join(".obsidian/app.json"), "{}").unwrap();
        mark_as_vault(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".obsidian/app.json")).unwrap(), "{}");
        std::fs::remove_dir_all(&loc).unwrap();
    }

    #[test]
    fn home_expansion() {
        if let Some(h) = std::env::var_os("HOME") {
            assert_eq!(expand_home("~/x"), PathBuf::from(h).join("x"));
        }
        assert_eq!(expand_home("/abs/~x"), PathBuf::from("/abs/~x"));
        assert_eq!(expand_home("~user/x"), PathBuf::from("~user/x"));
    }
}
