//! File recovery (Obsidian core plugin): local snapshots of notes, to get back text lost to a
//! bad edit, a sync conflict or a deleted note.
//!
//! Snapshots live outside the vault, on this machine only (your choice, M6): not synced,
//! like Obsidian's own (it keeps them in its local database). One folder per vault:
//!   $XDG_DATA_HOME/tephra/recovery/<vault name>-<hash of its path>/<hash of note path>/
//!     path          the note's vault path
//!     <unix ms>.md  one snapshot
//!
//! When: at most one snapshot per note per interval (default 5 min) while you edit; the text
//! from before your first edit is kept too; a change made outside the app (Syncthing, another
//! editor) always keeps the text it replaced. Snapshots older than the history length (default
//! 7 days) are removed at start-up.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};
use sha2::Digest as _;

use super::api::*;
use super::api::{Action, MenuItem};

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Unix time in milliseconds.
    pub ts: i64,
    pub path: PathBuf,
}

/// The snapshots of one vault.
#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where app data goes: `$TEPHRA_DATA_DIR`; inside the (throwaway) vault for scripted UI
/// tests; else `$XDG_DATA_HOME/tephra` (`~/.local/share/tephra`), on Windows
/// `%LOCALAPPDATA%\tephra`.
pub fn data_dir(vault: &Path) -> PathBuf {
    if let Some(d) = std::env::var_os("TEPHRA_DATA_DIR") {
        return PathBuf::from(d);
    }
    if std::env::var_os("TEPHRA_SCRIPT").is_some() || std::env::var_os("TEPHRA_BENCH").is_some() {
        return vault.join(".tephra/test-data");
    }
    let env_dir = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_absolute());
    // Windows has no $HOME/XDG: %LOCALAPPDATA%\tephra (machine-local, not roamed)
    let base = if cfg!(windows) { env_dir("LOCALAPPDATA").or_else(|| env_dir("APPDATA")) } else { None }
        .or_else(|| env_dir("XDG_DATA_HOME"))
        .unwrap_or_else(|| env_dir("HOME").or_else(|| env_dir("USERPROFILE")).unwrap_or_else(std::env::temp_dir).join(".local/share"));
    base.join("tephra")
}

impl Store {
    pub fn for_vault(vault: &Path) -> Self {
        let canon = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
        let name: String = canon.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect();
        let h = hex(&sha2::Sha256::digest(canon.to_string_lossy().as_bytes())[..6]);
        Self { dir: data_dir(vault).join("recovery").join(format!("{name}-{h}")) }
    }

    fn note_dir(&self, rel: &str) -> PathBuf {
        self.dir.join(hex(&sha2::Sha256::digest(rel.as_bytes())[..8]))
    }

    /// A note's snapshots, newest first.
    pub fn list(&self, rel: &str) -> Vec<Snapshot> {
        Self::list_dir(&self.note_dir(rel))
    }

    fn list_dir(d: &Path) -> Vec<Snapshot> {
        let mut v: Vec<Snapshot> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let ts = p.file_name()?.to_str()?.strip_suffix(".md")?.parse().ok()?;
                Some(Snapshot { ts, path: p })
            })
            .collect();
        v.sort_by(|a, b| b.ts.cmp(&a.ts));
        v
    }

    pub fn read(&self, s: &Snapshot) -> String {
        std::fs::read_to_string(&s.path).unwrap_or_default()
    }

    /// Saves `text` as a snapshot at `ts`, unless it equals the newest one. True if saved.
    pub fn save(&self, rel: &str, text: &str, ts: i64) -> std::io::Result<bool> {
        let d = self.note_dir(rel);
        if let Some(last) = self.list(rel).first() {
            if self.read(last) == text {
                return Ok(false);
            }
        }
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join("path"), rel)?;
        let tmp = d.join(format!("{ts}.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, d.join(format!("{ts}.md")))?;
        Ok(true)
    }

    /// The snapshot policy (see the module docs) for one change of a note.
    pub fn on_change(&self, rel: &str, old: Option<&str>, new: &str, external: bool, now: i64, interval_ms: i64) -> std::io::Result<()> {
        let snaps = self.list(rel);
        let latest_text = snaps.first().map(|s| self.read(s));
        // keep what was there before: before the first edit, and before any outside change
        if let Some(old) = old.filter(|o| *o != new) {
            if (external || snaps.is_empty()) && latest_text.as_deref() != Some(old) {
                self.save(rel, old, now - 1)?;
            }
        }
        let due = snaps.first().is_none_or(|s| now - s.ts >= interval_ms);
        if due && !external {
            self.save(rel, new, now)?;
        }
        Ok(())
    }

    /// Every note with snapshots: (vault path, count, newest), newest first.
    pub fn notes(&self) -> Vec<(String, usize, i64)> {
        let mut v: Vec<(String, usize, i64)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let rel = std::fs::read_to_string(e.path().join("path")).ok()?;
                let snaps = Self::list_dir(&e.path());
                let newest = snaps.first()?.ts;
                Some((rel, snaps.len(), newest))
            })
            .collect();
        v.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        v
    }

    /// Removes snapshots older than `keep_ms` (and folders left empty).
    pub fn prune(&self, now: i64, keep_ms: i64) {
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let snaps = Self::list_dir(&e.path());
            for s in snaps.iter().filter(|s| now - s.ts > keep_ms) {
                let _ = std::fs::remove_file(&s.path);
            }
            if Self::list_dir(&e.path()).is_empty() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn when(ts: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ts).map(|d| crate::moment::format("YYYY-MM-DD HH:mm:ss", &d.with_timezone(&chrono::Local).naive_local())).unwrap_or_default()
}

/// Lines only in `a` and only in `b` (as multisets): a cheap "+n −m" against the current text.
pub fn line_delta(a: &str, b: &str) -> (usize, usize) {
    let mut count: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for l in a.lines() {
        *count.entry(l).or_default() += 1;
    }
    for l in b.lines() {
        *count.entry(l).or_default() -= 1;
    }
    let plus = count.values().filter(|v| **v > 0).map(|v| *v as usize).sum();
    let minus = count.values().filter(|v| **v < 0).map(|v| (-*v) as usize).sum();
    (plus, minus)
}

pub struct FileRecovery {
    manifest: PluginManifest,
    store: Store,
}

impl FileRecovery {
    pub fn new(vault: &Path) -> Self {
        Self {
            manifest: PluginManifest {
                id: "file-recovery",
                name: "File recovery",
                version: "0.1.0",
                description: "Local snapshots of your notes (kept on this computer, not synced), to recover lost text.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "intervalMinutes", label: "Snapshot interval", description: "Minutes between two snapshots of a note while you edit it.", kind: SettingKind::Number, default: Value::from(5) },
                    SettingSpec { key: "keepDays", label: "History length", description: "Days snapshots are kept.", kind: SettingKind::Number, default: Value::from(7) },
                ],
                code_languages: &[],
            },
            store: Store::for_vault(vault),
        }
    }
}

struct Listener {
    store: Store,
    interval_ms: i64,
}

impl VaultListener for Listener {
    fn modified(&self, path: &str, old: Option<&str>, new: &str, external: bool, _cx: &mut App) {
        if let Err(e) = self.store.on_change(path, old, new, external, now_ms(), self.interval_ms) {
            eprintln!("file recovery: could not save a snapshot of {path}: {e}");
        }
    }
}

/// The history of one note: snapshots on the left, the chosen one's text on the right.
pub struct RecoveryView {
    store: Store,
    rel: String,
    snaps: Vec<Snapshot>,
    selected: usize,
    text: String,
    current: Option<String>,
}

impl RecoveryView {
    fn select(&mut self, ix: usize) {
        self.selected = ix;
        self.text = self.snaps.get(ix).map(|s| self.store.read(s)).unwrap_or_default();
    }

    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = WorkspaceHandle::current(cx) else { return };
        let text = self.text.clone();
        // the text being replaced is kept as a snapshot first
        if let Some(cur) = &self.current {
            let _ = self.store.save(&self.rel, cur, now_ms());
        }
        let res = if self.current.is_some() { ws.set_note_text(&self.rel, &text, cx) } else { ws.create_file(&self.rel, &text, cx).map(|_| ()) };
        match res {
            Ok(()) => {
                window.close_dialog(cx);
                ws.open(&self.rel, OpenAt::Keep, false, window, cx);
                window.push_notification(format!("Restored \"{}\" from {}.", self.rel, when(self.snaps[self.selected].ts)), cx);
            }
            Err(e) => window.push_notification(format!("Could not restore: {e}"), cx),
        }
    }
}

impl Render for RecoveryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = self
            .snaps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let delta = self.current.as_deref().map(|c| line_delta(&self.store.read(s), c));
                div()
                    .id(("snapshot", i))
                    .px_2()
                    .py_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(i == self.selected, |d| d.bg(theme.accent))
                    .child(div().text_sm().child(when(s.ts)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(match delta {
                        Some((0, 0)) => "same as now".to_string(),
                        Some((p, m)) => format!("+{p} −{m} lines vs now"),
                        None => "note deleted".to_string(),
                    }))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.select(i);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let text = self.text.clone();
        v_flex()
            .gap_2()
            .child(div().text_sm().text_color(theme.muted_foreground).child(format!("{} · {} snapshot(s), kept on this computer", self.rel, self.snaps.len())))
            .child(
                h_flex()
                    .h(crate::ui::r(440.))
                    .gap_3()
                    .items_start()
                    .child(div().id("snapshots").w(crate::ui::r(210.)).h_full().flex_none().overflow_y_scroll().children(rows))
                    .child(
                        div()
                            .id("snapshot-text")
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .overflow_y_scroll()
                            .p_2()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.muted.opacity(0.3))
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .whitespace_normal()
                            .children(text.lines().map(|l| div().min_h(px(14.)).child(SharedString::from(l.to_string()))).collect::<Vec<_>>()),
                    ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("copy-snapshot").label("Copy").outline().small().on_click(cx.listener(|v, _, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(v.text.clone()));
                        window.push_notification("Snapshot copied.", cx);
                    })))
                    .child(Button::new("restore-snapshot").label("Restore this version").primary().small().on_click(cx.listener(|v, _, window, cx| v.restore(window, cx)))),
            )
    }
}

/// Opens the history of `rel`.
pub fn open_history(store: &Store, rel: &str, window: &mut Window, cx: &mut App) {
    let snaps = store.list(rel);
    if snaps.is_empty() {
        window.push_notification(format!("No snapshots of \"{rel}\" yet."), cx);
        return;
    }
    let current = WorkspaceHandle::current(cx).and_then(|ws| ws.note_text(rel, cx));
    if std::env::var_os("TEPHRA_SCRIPT").is_some() {
        println!("RECOVERY {rel} {}", snaps.len());
    }
    let (store, rel) = (store.clone(), rel.to_string());
    let view = cx.new(|_| {
        let mut v = RecoveryView { store, rel, snaps, selected: 0, text: String::new(), current };
        v.select(0);
        v
    });
    window.open_dialog(cx, move |dialog, _, cx| dialog.title("File recovery").w(crate::ui::p(cx, 860.)).child(view.clone()));
}

impl Plugin for FileRecovery {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let num = |k: &str, d: f64| settings.get(k).and_then(Value::as_f64).filter(|v| *v >= 0.0).unwrap_or(d);
        let interval_ms = (num("intervalMinutes", 5.0) * 60_000.0) as i64;
        let keep_ms = (num("keepDays", 7.0) * 86_400_000.0) as i64;
        let store = self.store.clone();
        {
            let s = store.clone();
            std::thread::spawn(move || s.prune(now_ms(), keep_ms));
        }
        reg.vault_listener(Rc::new(Listener { store: store.clone(), interval_ms }));
        let s1 = store.clone();
        let open: Action = Rc::new(move |a: &mut ActionCtx| {
            let Some(rel) = a.workspace().and_then(|w| w.active_note(a.cx)) else { return };
            open_history(&s1, &rel, a.window, a.cx);
        });
        reg.command(Command { id: "open".into(), name: "Open local history of this note".into(), hotkey: None, run: open });
        let s2 = store.clone();
        reg.menu_item(MenuItem {
            id: "open".into(),
            label: "Open local history".into(),
            target: MenuTarget::File,
            run: Rc::new(move |a: &mut ActionCtx, rel: Option<&str>| {
                if let Some(rel) = rel {
                    open_history(&s2, rel, a.window, a.cx);
                }
            }),
        });
        let s3 = store.clone();
        reg.settings_tab(SettingsTab {
            id: "history".into(),
            title: "Snapshots".into(),
            render: Rc::new(move |_window, cx| {
                let theme = cx.theme().clone();
                let notes = s3.notes();
                let total: usize = notes.iter().map(|n| n.1).sum();
                let s4 = s3.clone();
                v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{} snapshot(s) of {} note(s), in {}", total, notes.len(), s3.dir.display())))
                    .children(notes.into_iter().take(200).enumerate().map(|(i, (rel, n, newest))| {
                        let (s, r) = (s3.clone(), rel.clone());
                        h_flex()
                            .gap_2()
                            .text_sm()
                            .child(div().flex_1().min_w_0().truncate().child(rel.clone()))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{n} · {}", when(newest))))
                            .child(Button::new(("view-history", i)).label("View").xsmall().ghost().on_click(move |_, window, cx| open_history(&s, &r, window, cx)))
                    }))
                    .child(h_flex().pt_2().child(Button::new("clear-history").label("Clear all snapshots").small().danger().on_click(move |_, window, cx| {
                        let s = s4.clone();
                        super::picker::confirm(
                            "Clear snapshots",
                            "Delete every snapshot of this vault from this computer? This cannot be undone.",
                            "Delete",
                            Rc::new(move |window, cx| {
                                s.clear();
                                window.refresh();
                                let _ = cx;
                            }),
                            window,
                            cx,
                        );
                    })))
                    .into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    // no `gpui_kit::*` glob here: its `test` macro would shadow the built-in one
    use super::{line_delta, now_ms, Store};

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!("ol-recovery-{}-{}", std::process::id(), now_ms()));
        Store { dir }
    }

    #[test]
    fn snapshot_policy() {
        let s = store();
        const MIN: i64 = 60_000;
        // first edit: the text before it, and the edit
        s.on_change("a.md", Some("v0"), "v1", false, 10 * MIN, 5 * MIN).unwrap();
        let texts = |s: &Store| s.list("a.md").iter().map(|x| s.read(x)).collect::<Vec<_>>();
        assert_eq!(texts(&s), ["v1", "v0"]);
        // within the interval: nothing new
        s.on_change("a.md", Some("v1"), "v2", false, 12 * MIN, 5 * MIN).unwrap();
        assert_eq!(texts(&s).len(), 2);
        // after it: the current text
        s.on_change("a.md", Some("v2"), "v3", false, 16 * MIN, 5 * MIN).unwrap();
        assert_eq!(texts(&s), ["v3", "v1", "v0"]);
        // an outside change keeps what it replaced, even within the interval
        s.on_change("a.md", Some("v4"), "synced", true, 17 * MIN, 5 * MIN).unwrap();
        assert_eq!(texts(&s), ["v4", "v3", "v1", "v0"]);
        assert_eq!(s.notes().iter().map(|n| (n.0.as_str(), n.1)).collect::<Vec<_>>(), [("a.md", 4)]);
        // pruning by age
        s.prune(17 * MIN, 3 * MIN);
        assert_eq!(texts(&s), ["v4", "v3"]);
        s.prune(10_000 * MIN, MIN);
        assert!(s.notes().is_empty());
        s.clear();
    }

    #[test]
    fn deltas() {
        assert_eq!(line_delta("a\nb\nc", "a\nc\nd\ne"), (1, 2));
        assert_eq!(line_delta("x", "x"), (0, 0));
    }
}
