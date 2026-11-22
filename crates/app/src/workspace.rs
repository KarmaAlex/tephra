//! The window: file tree sidebar, tabs, and the vault-wide watcher that keeps the index and
//! open editors in sync with the disk (Syncthing, other editors).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tree::{tree, TreeItem, TreeState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::index::{is_note, is_sync_conflict};
use obsidilight_core::links::link_path;

use crate::editor::{Editor, EditorEvent};
use crate::embeds::{SharedVault, Vault};

actions!(workspace, [CloseTab, NextTab, PrevTab, NewNote, OpenInNewTab, ToggleSidebar]);

pub const CONTEXT: &str = "Workspace";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("ctrl-w", CloseTab, c),
        KeyBinding::new("ctrl-tab", NextTab, c),
        KeyBinding::new("ctrl-shift-tab", PrevTab, c),
        KeyBinding::new("ctrl-n", NewNote, c),
        KeyBinding::new("ctrl-shift-b", ToggleSidebar, c),
    ]);
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
                        Ok(p) => p,
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

    // ---- watcher --------------------------------------------------------------------

    fn watch(&self, root: &Path, cx: &mut Context<Self>) -> Option<notify::RecommendedWatcher> {
        use notify::Watcher as _;
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
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
        let before = self.vault.borrow().index.files().count();
        for rel in rels {
            self.vault.borrow_mut().index.refresh(rel);
        }
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
    }

    // ---- rendering ------------------------------------------------------------------

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let weak = cx.entity().downgrade();
        let root = self.vault.borrow().root.clone();
        let target = self.menu_target.clone();
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
                .context_menu(move |_ix, entry, menu, _w, _cx| {
                    let id = entry.item().id.to_string();
                    let folder = entry.is_folder();
                    *target.borrow_mut() = Some((id.strip_prefix("dir:").unwrap_or(&id).to_string(), folder));
                    menu.when(!folder, |m| m.menu("Open in new tab", Box::new(OpenInNewTab))).menu("New note here", Box::new(NewNote))
                })
                .flex_1(),
            )
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
        let _ = window;
        let sidebar = self.sidebar.then(|| self.render_sidebar(cx).into_any_element());
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
                    }),
            )
    }
}
