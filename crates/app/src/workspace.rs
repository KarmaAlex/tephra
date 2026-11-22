//! The window: file tree sidebar, tabs, and the vault-wide watcher that keeps the index and
//! open editors in sync with the disk (Syncthing, other editors).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tree::{tree, TreeItem, TreeState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::index::{is_note, is_sync_conflict};
use obsidilight_core::links::link_path;

use crate::editor::{Editor, EditorEvent};
use crate::embeds::{SharedVault, Vault};
use crate::plugins::{run_action, MenuTarget, PluginHost};

actions!(workspace, [CloseTab, NextTab, PrevTab, NewNote, NewFolder, OpenInNewTab, RenameItem, DeleteItem, ToggleSidebar, ToggleRightSidebar, OpenSettings]);

pub const CONTEXT: &str = "Workspace";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("ctrl-w", CloseTab, c),
        KeyBinding::new("ctrl-tab", NextTab, c),
        KeyBinding::new("ctrl-shift-tab", PrevTab, c),
        KeyBinding::new("ctrl-n", NewNote, c),
        KeyBinding::new("ctrl-shift-b", ToggleSidebar, c),
        KeyBinding::new("ctrl-,", OpenSettings, c),
    ]);
}

/// `ctrl-alt-k` style hotkey vs a key press.
fn hotkey_matches(hotkey: &str, k: &Keystroke) -> bool {
    let parts: Vec<&str> = hotkey.split('-').collect();
    let Some((key, mods)) = parts.split_last() else { return false };
    let has = |m: &str| mods.iter().any(|x| x.eq_ignore_ascii_case(m));
    let m = &k.modifiers;
    key.eq_ignore_ascii_case(&k.key) && m.control == has("ctrl") && m.alt == has("alt") && m.shift == has("shift") && m.platform == (has("cmd") || has("super"))
}

pub struct Workspace {
    pub vault: SharedVault,
    tree: Entity<TreeState>,
    expanded: HashSet<String>,
    pub tabs: Vec<Entity<Editor>>,
    pub active: usize,
    focus: FocusHandle,
    /// The tree entry a context menu was opened on: (vault path, is folder).
    menu_target: Rc<RefCell<Option<(String, bool)>>>,
    sidebar: bool,
    /// Right sidebar (plugin views) shown, and which view.
    right_sidebar: bool,
    right_view: usize,
    /// Files shown in the tree after the last rebuild (for UI tests).
    pub tree_files: Vec<String>,
    _watcher: Option<notify::RecommendedWatcher>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let t = std::time::Instant::now();
        let vault = Rc::new(RefCell::new(Vault::open(root.clone())));
        if std::env::var_os("OBSIDILIGHT_TIMING").is_some() {
            eprintln!("TIMING vault+index {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
        let tree = cx.new(|cx| TreeState::new(cx));
        let subs = vec![cx.subscribe(&tree, |this, _, ev: &gpui_kit::component::tree::TreeEvent, _| {
            use gpui_kit::component::tree::TreeEvent;
            match ev {
                TreeEvent::Expanded(id) => {
                    this.expanded.insert(id.to_string());
                }
                TreeEvent::Collapsed(id) => {
                    this.expanded.remove(id.as_ref());
                }
            }
        })];
        let mut this = Self {
            vault,
            tree,
            expanded: HashSet::new(),
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            menu_target: Rc::default(),
            sidebar: true,
            right_sidebar: false,
            right_view: 0,
            tree_files: Vec::new(),
            _watcher: None,
            _subs: subs,
        };
        let t = std::time::Instant::now();
        this._watcher = this.watch(&root, cx);
        let tw = t.elapsed().as_secs_f64() * 1000.0;
        this.rebuild_tree(cx);
        if std::env::var_os("OBSIDILIGHT_TIMING").is_some() {
            eprintln!("TIMING watcher {tw:.1} ms, tree {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 - tw);
        }
        window.focus(&this.focus, cx);
        this
    }

    pub fn show_right_sidebar(&mut self, on: bool, cx: &mut Context<Self>) {
        self.right_sidebar = on;
        cx.notify();
    }

    pub fn active_editor(&self) -> Option<Entity<Editor>> {
        self.tabs.get(self.active).cloned()
    }

    /// The narrow strip on the far left: file tree toggle, plugin buttons, settings.
    fn render_ribbon(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let items: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.ribbon.iter().map(|(_, r)| r.clone()).collect()).unwrap_or_default();
        let has_views = cx.try_global::<PluginHost>().is_some_and(|h| !h.reg.sidebar_views.is_empty());
        let weak = cx.entity().downgrade();
        v_flex()
            .w(px(40.))
            .h_full()
            .flex_none()
            .items_center()
            .gap_1()
            .py_2()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(Button::new("ribbon-files").ghost().small().icon(gpui_kit::assets::IconName::Folder).tooltip("Files (Ctrl+Shift+B)").on_click(cx.listener(|ws, _, _, cx| {
                ws.sidebar = !ws.sidebar;
                cx.notify();
            })))
            .children(items.into_iter().enumerate().map(|(i, it)| {
                let weak = weak.clone();
                Button::new(("ribbon", i)).ghost().small().icon(it.icon).tooltip(it.tooltip.clone()).on_click(move |_, window, cx| {
                    let editor = weak.upgrade().and_then(|ws| ws.read(cx).active_editor());
                    run_action(&it.run, editor, window, cx);
                })
            }))
            .child(div().flex_1())
            .when(has_views, |d| {
                d.child(Button::new("ribbon-right").ghost().small().icon(gpui_kit::assets::IconName::PanelRight).tooltip("Right sidebar").on_click(cx.listener(|ws, _, _, cx| {
                    ws.right_sidebar = !ws.right_sidebar;
                    cx.notify();
                })))
            })
            .child(Button::new("ribbon-settings").ghost().small().icon(gpui_kit::assets::IconName::Settings).tooltip("Settings (Ctrl+,)").on_click(|_, window, cx| crate::settings_ui::open(window, cx)))
    }

    /// Plugin views on the right; a row of icons picks the view.
    fn render_right_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let views: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.sidebar_views.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default();
        if !self.right_sidebar || views.is_empty() {
            return None;
        }
        let ix = self.right_view.min(views.len() - 1);
        let theme = cx.theme().clone();
        let editor = self.active_editor();
        let body = (views[ix].render)(editor.as_ref(), window, cx);
        Some(
            v_flex()
                .w(px(300.))
                .h_full()
                .flex_none()
                .border_l_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .child(h_flex().gap_1().px_2().py_1().border_b_1().border_color(theme.border).children(views.iter().enumerate().map(|(i, v)| {
                    Button::new(("right-view", i)).ghost().xsmall().selected(i == ix).icon(v.icon).tooltip(v.title.clone()).on_click(cx.listener(move |ws, _, _, cx| {
                        ws.right_view = i;
                        cx.notify();
                    }))
                })))
                .child(div().flex_1().min_h_0().overflow_hidden().p_2().child(body))
                .into_any_element(),
        )
    }

    /// Plugin status items for the active editor.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let items: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.status.iter().map(|(_, s)| s.clone()).collect()).unwrap_or_default();
        let editor = self.active_editor();
        let texts: Vec<SharedString> = {
            let e = editor.as_ref().map(|e| e.read(cx));
            items.iter().filter_map(|s| (s.text)(e)).collect()
        };
        if texts.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .justify_end()
                .gap_4()
                .px_3()
                .py_0p5()
                .border_t_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .children(texts)
                .into_any_element(),
        )
    }

    /// Plugin command hotkeys (default bindings from their manifests' commands).
    fn on_key_capture(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let cmds: Vec<(String, crate::plugins::api::Action)> =
            cx.try_global::<PluginHost>().map(|h| h.reg.commands.iter().filter_map(|(_, c)| Some((c.hotkey.clone()?, c.run.clone()))).collect()).unwrap_or_default();
        let hit = cmds.into_iter().find(|(h, _)| hotkey_matches(h, &ev.keystroke)).map(|(_, r)| r);
        if let Some(run) = hit {
            let editor = self.active_editor();
            run_action(&run, editor, window, cx);
            cx.stop_propagation();
        }
    }

    fn rel(&self, p: &Path) -> String {
        self.vault.borrow().relative(p)
    }

    // ---- file tree ------------------------------------------------------------------

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        #[derive(Default)]
        struct Dir {
            dirs: BTreeMap<String, Dir>,
            files: Vec<String>,
        }
        let mut root = Dir::default();
        // folders from disk (so empty ones show), files from the index
        fn dirs(base: &Path, d: &Path, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(d) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && !e.file_name().to_string_lossy().starts_with('.') {
                    if let Ok(r) = p.strip_prefix(base) {
                        out.push(r.to_string_lossy().replace('\\', "/"));
                    }
                    dirs(base, &p, out);
                }
            }
        }
        let vroot = self.vault.borrow().root.clone();
        let mut all_dirs = Vec::new();
        dirs(&vroot, &vroot, &mut all_dirs);
        for dpath in all_dirs {
            let mut d = &mut root;
            for p in dpath.split('/') {
                d = d.dirs.entry(p.to_string()).or_default();
            }
        }
        self.tree_files = self.vault.borrow().index.files().cloned().collect();
        for f in self.vault.borrow().index.files() {
            let mut d = &mut root;
            let parts: Vec<&str> = f.split('/').collect();
            for p in &parts[..parts.len() - 1] {
                d = d.dirs.entry(p.to_string()).or_default();
            }
            d.files.push(f.clone());
        }
        fn items(d: &Dir, prefix: &str, expanded: &HashSet<String>) -> Vec<TreeItem> {
            // Obsidian's default order: folders first, then files, A to Z ignoring case
            let mut dirs: Vec<_> = d.dirs.iter().collect();
            dirs.sort_by_key(|(n, _)| n.to_lowercase());
            let mut files = d.files.clone();
            files.sort_by_key(|f| f.rsplit('/').next().unwrap_or(f).to_lowercase());
            let mut out = Vec::new();
            for (name, sub) in dirs {
                let id = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                out.push(TreeItem::new(format!("dir:{id}"), name.clone()).expanded(expanded.contains(&format!("dir:{id}"))).children(items(sub, &id, expanded)));
            }
            for f in files {
                let name = f.rsplit('/').next().unwrap_or(&f);
                let label = if is_note(name) { name[..name.len() - 3].to_string() } else { name.to_string() };
                out.push(TreeItem::new(f.clone(), label));
            }
            out
        }
        let list = items(&root, "", &self.expanded);
        self.tree.update(cx, |t, cx| t.set_items(list, cx));
        self.select_active_in_tree(cx);
    }

    fn select_active_in_tree(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.tabs.get(self.active) else { return };
        let rel = self.rel(&e.read(cx).path);
        // expand its folders (remembered, so a rebuild keeps them open), scroll to it, select it
        let mut dir = rel.as_str();
        while let Some((d, _)) = dir.rsplit_once('/') {
            self.expanded.insert(format!("dir:{d}"));
            dir = d;
        }
        let id: SharedString = rel.into();
        self.tree.update(cx, |t, cx| {
            t.reveal_item(&id, ScrollStrategy::Center, cx);
            if let Some(ix) = t.index_of(&id) {
                t.set_selected_index(Some(ix), cx);
            }
        });
    }

    // ---- tabs -----------------------------------------------------------------------

    /// Opens a note: focuses its tab if already open, else replaces the active tab (or adds one).
    pub fn open_path(&mut self, path: PathBuf, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) -> Entity<Editor> {
        if let Some(ix) = self.tabs.iter().position(|e| e.read(cx).path == path) {
            self.activate(ix, window, cx);
            return self.tabs[ix].clone();
        }
        let vault = self.vault.clone();
        let editor = cx.new(|cx| Editor::open(path, vault, window, cx));
        self._subs.push(cx.subscribe_in(&editor, window, Self::on_editor_event));
        if new_tab || self.tabs.is_empty() {
            self.tabs.push(editor.clone());
            self.active = self.tabs.len() - 1;
        } else {
            // leaving the replaced note: make sure its edits are on disk
            self.tabs[self.active].update(cx, |e, cx| e.save(cx));
            self.tabs[self.active] = editor.clone();
        }
        let f = editor.read(cx).focus.clone();
        window.focus(&f, cx);
        self.select_active_in_tree(cx);
        cx.notify();
        editor
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active = ix;
            let f = self.tabs[ix].read(cx).focus.clone();
            window.focus(&f, cx);
            self.select_active_in_tree(cx);
            cx.notify();
        }
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs[ix].update(cx, |e, cx| e.save(cx));
        self.tabs.remove(ix);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        if self.tabs.is_empty() {
            window.focus(&self.focus, cx);
        } else {
            let a = self.active;
            self.activate(a, window, cx);
        }
        cx.notify();
    }

    fn on_editor_event(&mut self, editor: &Entity<Editor>, ev: &EditorEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            EditorEvent::Saved(p) => {
                let rel = self.rel(p);
                self.vault.borrow_mut().index.refresh(&rel);
            }
            EditorEvent::OpenLink { target, new_tab } => {
                let from = editor.read(cx).path.clone();
                let sub = target.split_once('#').map(|(_, s)| s.to_string());
                let resolved = self.vault.borrow().resolve(target, &from);
                let path = match resolved {
                    Some(p) => p,
                    None => match self.create_note_for_link(target) {
                        Ok(p) => {
                            self.rebuild_tree(cx);
                            for t in self.tabs.clone() {
                                t.update(cx, |e, cx| e.invalidate(cx)); // the link is no longer unresolved
                            }
                            p
                        }
                        Err(e) => {
                            editor.update(cx, |ed, cx| {
                                ed.status = Some(format!("Could not create \"{target}\": {e}").into());
                                cx.notify();
                            });
                            return;
                        }
                    },
                };
                let ed = if path == from { editor.clone() } else { self.open_path(path, *new_tab, window, cx) };
                if let Some(sub) = sub.filter(|s| !s.is_empty()) {
                    ed.update(cx, |e, cx| e.go_to_subpath(&sub, cx));
                }
            }
        }
    }

    /// Ctrl+click on an unresolved link: create the note at the vault root (Obsidian's default
    /// location), including any folders in the link.
    fn create_note_for_link(&mut self, target: &str) -> std::io::Result<PathBuf> {
        let name = link_path(target).ok_or_else(|| std::io::Error::other("empty link"))?;
        let rel = if name.rsplit('/').next().is_some_and(|l| l.contains('.')) { name.to_string() } else { format!("{name}.md") };
        if rel.split('/').any(|c| c == ".." || c.is_empty()) {
            return Err(std::io::Error::other("invalid name"));
        }
        let path = self.vault.borrow().root.join(&rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        self.vault.borrow_mut().index.refresh(&rel);
        Ok(path)
    }

    /// New empty note "Untitled.md" ("Untitled 1.md"…) in `folder` (vault-relative), opened in a new tab.
    fn new_note_in(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.vault.borrow().root.clone();
        let dir = root.join(folder);
        let mut name = "Untitled.md".to_string();
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("Untitled {n}.md");
            n += 1;
        }
        let path = dir.join(&name);
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::OpenOptions::new().write(true).create_new(true).open(&path)).is_ok() {
            let rel = self.rel(&path);
            self.vault.borrow_mut().index.refresh(&rel);
            self.rebuild_tree(cx);
            self.open_path(path, true, window, cx);
        }
    }

    // ---- file operations --------------------------------------------------------------

    fn notify_status(&self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        let msg = msg.into();
        println!("STATUS {msg}");
        if let Some(e) = self.tabs.get(self.active) {
            e.update(cx, |e, cx| {
                e.status = Some(msg.clone());
                cx.notify();
                // informational: hide after a moment unless something else replaced it
                cx.spawn(async move |e, cx| {
                    cx.background_executor().timer(Duration::from_millis(2500)).await;
                    let _ = e.update(cx, |e, cx| {
                        if e.status.as_ref() == Some(&msg) {
                            e.status = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            });
        }
    }

    /// Renames or moves a file or folder (vault paths) and rewrites every link that pointed at
    /// what moved, so links keep working (HANDOFF §5.1).
    pub fn rename_path(&mut self, old: &str, new: &str, cx: &mut Context<Self>) -> Result<(usize, usize), String> {
        let new = new.trim().trim_matches('/').to_string();
        if new.is_empty() || new.split('/').any(|c| c.is_empty() || c == "." || c == ".." || c.starts_with('.')) {
            return Err("invalid name".into());
        }
        if new == old {
            return Ok((0, 0));
        }
        let root = self.vault.borrow().root.clone();
        let (old_abs, new_abs) = (root.join(old), root.join(&new));
        if new_abs.exists() && !old.eq_ignore_ascii_case(&new) {
            return Err(format!("\"{new}\" already exists"));
        }
        // 1. unsaved edits first, so the index (and the plan's byte ranges) match the disk
        for e in &self.tabs {
            e.update(cx, |e, cx| e.save(cx));
        }
        if self.tabs.iter().any(|e| e.read(cx).doc.is_dirty()) {
            return Err("a note has unsaved changes that could not be saved".into());
        }
        // 2. which files move, and the link edits that keeps links intact
        let moves: Vec<(String, String)> = if old_abs.is_dir() {
            let prefix = format!("{old}/");
            self.vault.borrow().index.files().filter(|f| f.starts_with(&prefix)).map(|f| (f.clone(), format!("{new}/{}", &f[prefix.len()..]))).collect()
        } else {
            vec![(old.to_string(), new.clone())]
        };
        let plan = self.vault.borrow().index.rename_plan(&moves);
        // 3. move on disk
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&old_abs, &new_abs).map_err(|e| e.to_string())?;
        for e in &self.tabs {
            let p = e.read(cx).path.clone();
            let rel = self.rel(&p);
            if let Some((_, n)) = moves.iter().find(|(o, _)| *o == rel) {
                let np = root.join(n);
                e.update(cx, |e, cx| e.set_path(np, cx));
            }
        }
        // 4. rewrite links: through the editor if the note is open, else straight to disk
        let mut links = 0;
        let mut failures = Vec::new();
        for edit in &plan {
            links += edit.edits.len();
            let path = root.join(&edit.note);
            if let Some(e) = self.tabs.iter().find(|e| e.read(cx).path == path) {
                e.update(cx, |e, cx| e.apply_edits(&edit.edits, cx));
                continue;
            }
            let res = obsidilight_core::file::load(&path).and_then(|l| {
                let text = obsidilight_core::index::apply_edits(&l.text, &edit.edits);
                obsidilight_core::file::save(&path, &text, Some(l.disk))
            });
            if !matches!(res, Ok(obsidilight_core::file::SaveOutcome::Saved(_) | obsidilight_core::file::SaveOutcome::Unchanged(_))) {
                failures.push(edit.note.clone());
            }
        }
        // 5. re-index
        let fresh = obsidilight_core::index::VaultIndex::build(&root);
        self.vault.borrow_mut().index = fresh;
        if old_abs.is_dir() || new_abs.is_dir() {
            self.expanded.insert(format!("dir:{new}"));
        }
        self.rebuild_tree(cx);
        for e in &self.tabs {
            e.update(cx, |e, cx| e.invalidate(cx));
        }
        cx.notify();
        if failures.is_empty() {
            Ok((links, plan.len()))
        } else {
            Err(format!("moved, but links in {} note(s) could not be updated: {}", failures.len(), failures.join(", ")))
        }
    }

    /// Moves a file or folder to the system trash (Obsidian's default), closing its tabs.
    pub fn delete_path(&mut self, rel: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let root = self.vault.borrow().root.clone();
        let abs = root.join(rel);
        let doomed: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, e)| e.read(cx).path.starts_with(&abs)).map(|(i, _)| i).collect();
        for &i in doomed.iter().rev() {
            self.tabs[i].update(cx, |e, _| e.disable_saving());
            self.close_tab(i, window, cx);
        }
        trash::delete(&abs).map_err(|e| e.to_string())?;
        let fresh = obsidilight_core::index::VaultIndex::build(&root);
        self.vault.borrow_mut().index = fresh;
        self.rebuild_tree(cx);
        for e in &self.tabs {
            e.update(cx, |e, cx| e.invalidate(cx));
        }
        cx.notify();
        Ok(())
    }

    fn create_folder(&mut self, rel: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let rel = rel.trim().trim_matches('/');
        if rel.is_empty() || rel.split('/').any(|c| c.is_empty() || c == ".." || c.starts_with('.')) {
            return Err("invalid name".into());
        }
        let abs = self.vault.borrow().root.join(rel);
        std::fs::create_dir_all(&abs).map_err(|e| e.to_string())?;
        // empty folders have no files in the index; keep it visible via a placeholder-free rebuild
        self.expanded.insert(format!("dir:{rel}"));
        self.rebuild_tree(cx);
        Ok(())
    }

    fn rename_dialog(&mut self, rel: String, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let shown = if !folder && is_note(&rel) { rel[..rel.len() - 3].to_string() } else { rel.clone() };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(shown));
        let weak = cx.entity().downgrade();
        let focus = input.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let weak = weak.clone();
            let rel = rel.clone();
            dialog
                .title(if folder { "Rename or move folder" } else { "Rename or move note" })
                .child(v_flex().gap_2().child(Input::new(&input)).child(div().text_xs().child("Edit the path to move it to another folder. Links to it are updated.")))
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().child(Button::new("cancel").label("Cancel").outline()))
                        .child(DialogAction::new().child(Button::new("ok").label("Rename").primary())),
                )
                .on_ok(move |_, window, cx| {
                    let mut new = input.read(cx).value().to_string();
                    if !folder && is_note(&rel) && !new.to_lowercase().ends_with(".md") {
                        new.push_str(".md");
                    }
                    let res = weak.update(cx, |ws, cx| ws.rename_path(&rel, &new, cx));
                    match res {
                        Ok(Ok((links, notes))) => {
                            let _ = weak.update(cx, |ws, cx| ws.notify_status(format!("Renamed. Updated {links} link(s) in {notes} note(s)."), cx));
                            true
                        }
                        Ok(Err(e)) => {
                            window.push_notification(format!("Rename failed: {e}"), cx);
                            false
                        }
                        Err(_) => true,
                    }
                })
        });
        window.focus(&focus, cx);
    }

    fn new_folder_dialog(&mut self, parent: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        let weak = cx.entity().downgrade();
        let focus = input.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let weak = weak.clone();
            let parent = parent.clone();
            dialog
                .title("New folder")
                .child(Input::new(&input))
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().child(Button::new("cancel").label("Cancel").outline()))
                        .child(DialogAction::new().child(Button::new("ok").label("Create").primary())),
                )
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value().to_string();
                    let rel = if parent.is_empty() { name } else { format!("{parent}/{name}") };
                    match weak.update(cx, |ws, cx| ws.create_folder(&rel, cx)) {
                        Ok(Err(e)) => {
                            window.push_notification(format!("Could not create folder: {e}"), cx);
                            false
                        }
                        _ => true,
                    }
                })
        });
        window.focus(&focus, cx);
    }

    fn delete_dialog(&mut self, rel: String, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let what = if folder { format!("the folder \"{rel}\" and everything in it") } else { format!("\"{rel}\"") };
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            let rel = rel.clone();
            dialog
                .title("Delete")
                .child(format!("Move {what} to the system trash? You can restore it from the trash."))
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().child(Button::new("cancel").label("Cancel").outline()))
                        .child(DialogAction::new().child(Button::new("ok").label("Move to trash").danger())),
                )
                .on_ok(move |_, window, cx| {
                    if let Ok(Err(e)) = weak.update(cx, |ws, cx| ws.delete_path(&rel, window, cx)) {
                        window.push_notification(format!("Delete failed: {e}"), cx);
                    }
                    true
                })
        });
    }

    // ---- watcher --------------------------------------------------------------------

    fn watch(&self, root: &Path, cx: &mut Context<Self>) -> Option<notify::RecommendedWatcher> {
        use notify::Watcher as _;
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                use notify::event::{EventKind, ModifyKind};
                // Only real changes. Access events (open/read/close) and metadata changes must be
                // ignored: re-indexing reads files and listing folders opens them, so reacting to
                // those would re-trigger itself forever (this caused ~1 s UI stalls every second).
                let relevant = match ev.kind {
                    EventKind::Create(_) | EventKind::Remove(_) => true,
                    EventKind::Modify(ModifyKind::Metadata(_)) => false,
                    EventKind::Modify(_) => true,
                    EventKind::Access(_) | EventKind::Other | EventKind::Any => false,
                };
                if !relevant {
                    return;
                }
                if std::env::var_os("OBSIDILIGHT_TIMING").is_some() {
                    eprintln!("EVENT {:?} {:?}", ev.kind, ev.paths.first().and_then(|p| p.file_name()));
                }
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        })
        .ok()?;
        w.watch(root, notify::RecursiveMode::Recursive).ok()?;
        let root = root.to_path_buf();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let mut changed: Vec<String> = Vec::new();
            while let Ok(p) = rx.try_recv() {
                let Ok(rel) = p.strip_prefix(&root) else { continue };
                let rel = rel.to_string_lossy().replace('\\', "/");
                // our own temp files and dot-folders (.obsidian, .obsidilight, .stfolder…)
                if rel.is_empty() || rel.split('/').any(|c| c.starts_with('.')) || rel.contains(".obsidilight-tmp-") {
                    continue;
                }
                if !changed.contains(&rel) {
                    changed.push(rel);
                }
            }
            if changed.is_empty() {
                continue;
            }
            if this.update(cx, |ws, cx| ws.on_files_changed(&changed, cx)).is_err() {
                break;
            }
        })
        .detach();
        Some(w)
    }

    fn on_files_changed(&mut self, rels: &[String], cx: &mut Context<Self>) {
        let t = std::time::Instant::now();
        let log = std::env::var_os("OBSIDILIGHT_TIMING").is_some();
        let before = self.vault.borrow().index.files().count();
        self.vault.borrow_mut().index.refresh_batch(rels);
        let root = self.vault.borrow().root.clone();
        for e in &self.tabs {
            let p = e.read(cx).path.clone();
            if rels.iter().any(|r| root.join(r) == p) {
                e.update(cx, |e, cx| e.on_disk_changed(cx));
            }
        }
        let after = self.vault.borrow().index.files().count();
        if before != after || rels.iter().any(|r| !root.join(r).exists()) {
            self.rebuild_tree(cx);
        }
        // links may now resolve differently: repaint editors
        for e in &self.tabs {
            e.update(cx, |e, cx| e.invalidate(cx));
        }
        cx.notify();
        if log {
            eprintln!("TIMING files_changed {} paths ({:?}…) took {:.1} ms", rels.len(), rels.first(), t.elapsed().as_secs_f64() * 1000.0);
        }
    }

    // ---- rendering ------------------------------------------------------------------

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let weak = cx.entity().downgrade();
        let root = self.vault.borrow().root.clone();
        let target = self.menu_target.clone();
        let menu_ws = cx.entity().downgrade();
        v_flex()
            .w(px(280.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(h_flex().px_3().py_2().justify_between().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Files")).child(
                Button::new("new-note").ghost().xsmall().icon(IconName::Plus).tooltip("New note (Ctrl+N)").on_click(cx.listener(|ws, _, w, cx| ws.new_note_in("", w, cx))),
            ))
            .child({
                let root_target = self.menu_target.clone();
                // right-click on empty space (below the items, or the strip at the bottom):
                // the vault root
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                    tree(&self.tree, move |ix, entry, _selected, _w, cx| {
                        let item = entry.item();
                        let id = item.id.to_string();
                        let folder = entry.is_folder();
                        let conflict = !folder && is_sync_conflict(&id);
                        let icon = if folder {
                            if entry.is_expanded() { IconName::FolderOpen } else { IconName::Folder }
                        } else if conflict {
                            IconName::TriangleAlert
                        } else {
                            IconName::File
                        };
                        let theme = cx.theme();
                        let (fg, warn) = (theme.foreground, theme.warning);
                        let weak = weak.clone();
                        let path = root.join(&id);
                        ListItem::new(ix)
                            .w_full()
                            .px_2()
                            .pl(px(14.) * entry.depth() + px(8.))
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .text_sm()
                                    .text_color(if conflict { warn } else { fg })
                                    .child(Icon::new(icon).small())
                                    .child(item.label.clone())
                                    .when(conflict, |d| d.child(div().text_xs().child("sync conflict"))),
                            )
                            .when(!folder, |li| {
                                li.on_click(move |ev, window, cx| {
                                    let new_tab = ev.modifiers().secondary(cx);
                                    let p = path.clone();
                                    let _ = weak.update(cx, |ws, cx| {
                                        ws.open_path(p, new_tab, window, cx);
                                    });
                                })
                            })
                    })
                    .context_menu(move |_ix, entry, menu, _w, cx| {
                        let id = entry.item().id.to_string();
                        let folder = entry.is_folder();
                        let rel = id.strip_prefix("dir:").unwrap_or(&id).to_string();
                        *target.borrow_mut() = Some((rel.clone(), folder));
                        let mut menu = menu
                            .when(!folder, |m| m.menu("Open in new tab", Box::new(OpenInNewTab)))
                            .menu("New note here", Box::new(NewNote))
                            .menu("New folder here", Box::new(NewFolder))
                            .separator()
                            .menu("Rename or move…", Box::new(RenameItem))
                            .menu("Delete", Box::new(DeleteItem));
                        let items: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.menu_items.iter().filter(|(_, m)| m.target == MenuTarget::File).map(|(_, m)| m.clone()).collect()).unwrap_or_default();
                        if !items.is_empty() {
                            menu = menu.separator();
                        }
                        for it in items {
                            let rel = rel.clone();
                            let weak = menu_ws.clone();
                            menu = menu.item(gpui_kit::component::menu::PopupMenuItem::new(it.label.clone()).on_click(move |_, window, cx| {
                                let editor = weak.upgrade().and_then(|ws| ws.read(cx).active_editor());
                                let mut ctx = crate::plugins::ActionCtx { window, cx, editor };
                                (it.run)(&mut ctx, Some(&rel));
                            }));
                        }
                        menu
                    })
                    .flex_1(),
                    )
                    .child(div().h(px(48.)).flex_none())
                    .context_menu(move |menu, _, _| {
                        *root_target.borrow_mut() = None;
                        menu.menu("New note", Box::new(NewNote)).menu("New folder", Box::new(NewFolder))
                    })
            })
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let names: Vec<(SharedString, bool)> = self
            .tabs
            .iter()
            .map(|e| {
                let e = e.read(cx);
                (e.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().into(), e.doc.is_dirty())
            })
            .collect();
        let active = self.tabs.get(self.active).cloned();
        let sidebar = self.sidebar.then(|| self.render_sidebar(cx).into_any_element());
        let ribbon = self.render_ribbon(cx).into_any_element();
        let right = self.render_right_sidebar(window, cx);
        let status = self.render_status_bar(cx);
        div()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(|ws, _: &CloseTab, w, cx| {
                let a = ws.active;
                ws.close_tab(a, w, cx)
            }))
            .on_action(cx.listener(|ws, _: &NextTab, w, cx| {
                let n = ws.tabs.len().max(1);
                let a = (ws.active + 1) % n;
                ws.activate(a, w, cx)
            }))
            .on_action(cx.listener(|ws, _: &PrevTab, w, cx| {
                let n = ws.tabs.len().max(1);
                let a = (ws.active + n - 1) % n;
                ws.activate(a, w, cx)
            }))
            .capture_key_down(cx.listener(Self::on_key_capture))
            .on_action(cx.listener(|_, _: &OpenSettings, w, cx| crate::settings_ui::open(w, cx)))
            .on_action(cx.listener(|ws, _: &ToggleRightSidebar, _, cx| {
                ws.right_sidebar = !ws.right_sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ToggleSidebar, _, cx| {
                ws.sidebar = !ws.sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &NewNote, w, cx| {
                // from the tree's context menu: that folder; otherwise the vault root
                let target = ws.menu_target.borrow_mut().take();
                let folder = match target {
                    Some((p, true)) => p,
                    Some((p, false)) => p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(),
                    None => String::new(),
                };
                ws.new_note_in(&folder, w, cx)
            }))
            .on_action(cx.listener(|ws, _: &OpenInNewTab, w, cx| {
                let target = ws.menu_target.borrow_mut().take();
                if let Some((p, false)) = target {
                    let path = ws.vault.borrow().root.join(p);
                    ws.open_path(path, true, w, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &NewFolder, w, cx| {
                let target = ws.menu_target.borrow_mut().take();
                let parent = match target {
                    Some((p, true)) => p,
                    Some((p, false)) => p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(),
                    None => String::new(),
                };
                ws.new_folder_dialog(parent, w, cx)
            }))
            .on_action(cx.listener(|ws, _: &RenameItem, w, cx| {
                let target = ws.menu_target.borrow_mut().take();
                if let Some((p, folder)) = target {
                    ws.rename_dialog(p, folder, w, cx)
                }
            }))
            .on_action(cx.listener(|ws, _: &DeleteItem, w, cx| {
                let target = ws.menu_target.borrow_mut().take();
                if let Some((p, folder)) = target {
                    ws.delete_dialog(p, folder, w, cx)
                }
            }))
            .child(ribbon)
            .children(sidebar)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(!names.is_empty(), |d| {
                        d.child(
                            TabBar::new("tabs")
                                .selected_index(self.active)
                                .on_click(cx.listener(|ws, ix: &usize, w, cx| ws.activate(*ix, w, cx)))
                                .children(names.into_iter().enumerate().map(|(ix, (name, dirty))| {
                                    Tab::new().label(if dirty { format!("{name} •").into() } else { name }).suffix(
                                        Button::new(("close-tab", ix)).ghost().xsmall().icon(IconName::Close).on_click(cx.listener(move |ws, _, w, cx| ws.close_tab(ix, w, cx))),
                                    )
                                })),
                        )
                    })
                    .child(match active {
                        Some(e) => div().flex_1().min_h_0().child(e).into_any_element(),
                        None => div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme.muted_foreground)
                            .child("No file open — pick one on the left, or Ctrl+N for a new note")
                            .into_any_element(),
                    })
                    .children(status),
            )
            .children(right)
    }
}
