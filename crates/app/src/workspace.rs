//! The window: file tree sidebar, tabs, and the vault-wide watcher that keeps the index and
//! open editors in sync with the disk (Syncthing, other editors).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tab::{Tab as TabButton, TabBar};
use gpui_kit::component::tree::{tree, TreeItem, TreeState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::index::{is_note, is_sync_conflict};
use obsidilight_core::links::link_path;

use crate::editor::{Editor, EditorEvent};
use crate::embeds::{SharedVault, Vault};
use crate::plugins::api::{OpenAt, Side, ViewCtx, WorkspaceHandle};
use crate::plugins::{run_action, MenuTarget, PluginHost};

actions!(workspace, [CloseTab, NextTab, PrevTab, NewNote, NewFolder, OpenInNewTab, RenameItem, DeleteItem, ToggleSidebar, ToggleRightSidebar, OpenSettings]);

pub const CONTEXT: &str = "Workspace";

/// The workspace's keys are its commands' hotkeys (`commands.rs`, run by `hotkeys.rs`), so
/// you can change them; nothing is bound here.
pub fn bind_keys(_cx: &mut App) {}

/// The arrangement of panes: a pane, or a row (`Horizontal`) / column (`Vertical`) of them.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(usize),
    Split { id: usize, axis: Axis, children: Vec<Layout> },
}

impl Layout {
    fn panes(&self, out: &mut Vec<usize>) {
        match self {
            Layout::Pane(p) => out.push(*p),
            Layout::Split { children, .. } => children.iter().for_each(|c| c.panes(out)),
        }
    }

    /// Puts pane `new` next to pane `at` along `axis` (a sibling if that split runs the same
    /// way, else a new split around `at`).
    fn insert_after(&mut self, at: usize, new: usize, axis: Axis, split_id: usize) -> bool {
        match self {
            Layout::Pane(p) if *p == at => {
                *self = Layout::Split { id: split_id, axis, children: vec![Layout::Pane(at), Layout::Pane(new)] };
                true
            }
            Layout::Pane(_) => false,
            Layout::Split { axis: a, children, .. } => {
                if *a == axis {
                    if let Some(i) = children.iter().position(|c| *c == Layout::Pane(at)) {
                        children.insert(i + 1, Layout::Pane(new));
                        return true;
                    }
                }
                children.iter_mut().any(|c| c.insert_after(at, new, axis, split_id))
            }
        }
    }

    /// Removes pane `p`; splits left with one child collapse into it.
    fn remove(&mut self, p: usize) {
        if let Layout::Split { children, .. } = self {
            children.retain(|c| *c != Layout::Pane(p));
            for c in children.iter_mut() {
                c.remove(p);
            }
            if children.len() == 1 {
                *self = children.remove(0);
            }
        }
    }
}

/// A tab: a note's editor, or a view a plugin opened in a tab (the graph).
#[derive(Clone)]
pub enum Tab {
    Note(Entity<Editor>),
    View(ViewTab),
}

#[derive(Clone)]
pub struct ViewTab {
    /// The tab view type (`<plugin>:<id>`); the tab closes when its plugin is turned off.
    pub kind: String,
    pub arg: Option<String>,
    pub title: SharedString,
    pub view: AnyView,
    pub focus: FocusHandle,
}

impl Tab {
    pub fn editor(&self) -> Option<&Entity<Editor>> {
        match self {
            Tab::Note(e) => Some(e),
            Tab::View(_) => None,
        }
    }
    fn focus(&self, cx: &App) -> FocusHandle {
        match self {
            Tab::Note(e) => e.read(cx).focus.clone(),
            Tab::View(v) => v.focus.clone(),
        }
    }
    /// (title, unsaved).
    fn label(&self, cx: &App) -> (SharedString, bool) {
        match self {
            Tab::Note(e) => {
                let e = e.read(cx);
                // notes by name; other files by their full file name (`style.css`)
                let name = if e.plain { e.path.file_name() } else { e.path.file_stem() };
                (name.map(|s| s.to_string_lossy().to_string()).unwrap_or_default().into(), e.doc.is_dirty())
            }
            Tab::View(v) => (v.title.clone(), false),
        }
    }
    fn save(&self, cx: &mut App) {
        if let Tab::Note(e) = self {
            e.update(cx, |e, cx| e.save(cx));
        }
    }
}

/// A hover preview shown for a sidebar row.
struct WsPreview {
    key: String,
    rel: String,
    at: Point<Pixels>,
    text: Rc<str>,
    snippet: String,
    numbers: HashMap<usize, String>,
}

pub struct Workspace {
    pub vault: SharedVault,
    tree: Entity<TreeState>,
    expanded: HashSet<String>,
    /// Tabs of the focused pane, and its active tab. Other panes wait in `parked` (focusing a
    /// pane swaps it in), so tab code only ever deals with the focused pane.
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Id of the focused pane.
    pub pane: usize,
    parked: BTreeMap<usize, (Vec<Tab>, usize)>,
    /// How panes are arranged (split right / split down).
    layout: Layout,
    next_pane: usize,
    focus: FocusHandle,
    /// The tree entry a context menu was opened on: (vault path, is folder).
    menu_target: Rc<RefCell<Option<(String, bool)>>>,
    sidebar: bool,
    /// Sidebars as they were before focus mode (Some = in focus mode).
    /// Focus mode: (left, right sidebar as they were, fullscreen was switched on by it).
    focus_mode: Option<(bool, bool, bool)>,
    /// Left sidebar view: None = Files, else a plugin view id (`search:search`).
    left_view: Option<String>,
    /// Right sidebar (plugin views) shown, and which view (None = the first).
    right_sidebar: bool,
    right_view: Option<String>,
    /// Arguments for views, handed to their next render (`WorkspaceHandle::show_view`).
    view_args: HashMap<String, String>,
    /// Recently opened notes, most recent first (vault paths; the quick switcher shows them).
    pub recent: Vec<String>,
    /// Files shown in the tree after the last rebuild (for UI tests).
    pub tree_files: Vec<String>,
    /// Hover preview requested by a sidebar list (key = `rel#sub`), and the one shown.
    preview_pending: Option<String>,
    preview: Option<WsPreview>,
    /// Document version of each editor last propagated to its peers (live sync).
    synced: HashMap<EntityId, u64>,
    _watcher: Option<Rc<crate::watcher::VaultWatcher>>,
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
            pane: 0,
            parked: BTreeMap::new(),
            layout: Layout::Pane(0),
            next_pane: 1,
            focus: cx.focus_handle(),
            menu_target: Rc::default(),
            sidebar: true,
            focus_mode: None,
            left_view: None,
            right_sidebar: false,
            right_view: None,
            view_args: HashMap::new(),
            recent: Vec::new(),
            tree_files: Vec::new(),
            synced: HashMap::new(),
            preview_pending: None,
            preview: None,
            _watcher: None,
            _subs: subs,
        };
        WorkspaceHandle::install(cx.entity().downgrade(), cx);
        crate::commands::register_core(cx);
        this.load_layout();
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

    /// Every open editor, in every pane.
    pub fn all_editors(&self) -> Vec<Entity<Editor>> {
        let mut v: Vec<Entity<Editor>> = self.tabs.iter().filter_map(|t| t.editor().cloned()).collect();
        for (tabs, _) in self.parked.values() {
            v.extend(tabs.iter().filter_map(|t| t.editor().cloned()));
        }
        v
    }

    /// The tabs of pane `id`.
    pub fn pane_tabs(&self, id: usize) -> Vec<Tab> {
        if id == self.pane { self.tabs.clone() } else { self.parked.get(&id).map(|p| p.0.clone()).unwrap_or_default() }
    }

    /// Pane ids in layout order.
    pub fn pane_ids(&self) -> Vec<usize> {
        let mut v = Vec::new();
        self.layout.panes(&mut v);
        v
    }

    /// Makes pane `id` the focused one (its tabs become `tabs`).
    pub fn focus_pane(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if id == self.pane {
            return;
        }
        let Some((tabs, active)) = self.parked.remove(&id) else { return };
        let old = (std::mem::replace(&mut self.tabs, tabs), std::mem::replace(&mut self.active, active));
        self.parked.insert(self.pane, old);
        self.pane = id;
        if let Some(t) = self.tabs.get(self.active) {
            let f = t.focus(cx);
            window.focus(&f, cx);
        }
        self.select_active_in_tree(cx);
        cx.notify();
    }

    /// Splits the focused pane (right = `Horizontal`, down = `Vertical`); the new pane shows the
    /// same note and gets the focus.
    pub fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let new = self.next_pane;
        self.next_pane += 2;
        if !self.layout.insert_after(self.pane, new, axis, new + 1) {
            return;
        }
        let note = self.active_editor().map(|e| e.read(cx).path.clone());
        self.parked.insert(new, (Vec::new(), 0));
        self.focus_pane(new, window, cx);
        if let Some(p) = note {
            self.open_path(p, true, window, cx);
        }
        cx.notify();
    }

    /// Focuses the next pane in layout order (wrapping).
    pub fn focus_next_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.pane_ids();
        if let Some(i) = ids.iter().position(|p| *p == self.pane) {
            let next = ids[(i + 1) % ids.len()];
            self.focus_pane(next, window, cx);
        }
    }

    /// Closes the focused pane (its tabs are saved first); the last pane stays.
    pub fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.pane_ids();
        if ids.len() < 2 {
            return;
        }
        for t in self.tabs.clone() {
            t.save(cx);
        }
        let gone = self.pane;
        let i = ids.iter().position(|p| *p == gone).unwrap_or(0);
        let next = if i > 0 { ids[i - 1] } else { ids[1] };
        self.focus_pane(next, window, cx);
        self.parked.remove(&gone);
        self.layout.remove(gone);
        cx.notify();
    }

    pub fn active_editor(&self) -> Option<Entity<Editor>> {
        self.tabs.get(self.active).and_then(|t| t.editor().cloned())
    }

    /// The narrow strip on the far left: file tree toggle, plugin buttons, settings.
    fn render_ribbon(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let items: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.ribbon.iter().map(|(_, r)| r.clone()).collect()).unwrap_or_default();
        let has_views = cx.try_global::<PluginHost>().is_some_and(|h| h.reg.sidebar_views.iter().any(|(_, v)| v.side == Side::Right));
        let weak = cx.entity().downgrade();
        v_flex()
            .w(crate::ui::r(crate::ui::RIBBON))
            .h_full()
            .flex_none()
            .items_center()
            .gap_1()
            .py_2()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(crate::ui::icon_button("ribbon-files", gpui_kit::assets::IconName::PanelLeft).tooltip("Left sidebar (Ctrl+Shift+B)").on_click(cx.listener(|ws, _, _, cx| {
                ws.sidebar = !ws.sidebar;
                ws.save_layout();
                cx.notify();
            })))
            .children(items.into_iter().enumerate().map(|(i, it)| {
                let weak = weak.clone();
                crate::ui::icon_button(("ribbon", i), it.icon).tooltip(it.tooltip.clone()).on_click(move |_, window, cx| {
                    let editor = weak.upgrade().and_then(|ws| ws.read(cx).active_editor());
                    run_action(&it.run, editor, window, cx);
                })
            }))
            .child(div().flex_1())
            .when(has_views, |d| {
                d.child(crate::ui::icon_button("ribbon-right", gpui_kit::assets::IconName::PanelRight).tooltip("Right sidebar").on_click(cx.listener(|ws, _, _, cx| {
                    ws.right_sidebar = !ws.right_sidebar;
                    ws.save_layout();
                    cx.notify();
                })))
            })
            .child(crate::ui::icon_button("ribbon-settings", gpui_kit::assets::IconName::Settings).tooltip("Settings (Ctrl+,)").on_click(|_, window, cx| crate::settings_ui::open(window, cx)))
    }

    /// Plugin views on one side.
    fn views(cx: &App, side: Side) -> Vec<crate::plugins::SidebarView> {
        cx.try_global::<PluginHost>().map(|h| h.reg.sidebar_views.iter().filter(|(_, v)| v.side == side).map(|(_, v)| v.clone()).collect()).unwrap_or_default()
    }

    /// Renders one plugin view, handing over a pending argument.
    fn render_view(&mut self, v: &crate::plugins::SidebarView, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.active_editor();
        let active_note = editor.as_ref().map(|e| self.rel(&e.read(cx).path));
        let ctx = ViewCtx { editor, workspace: WorkspaceHandle(cx.entity().downgrade()), vault: self.vault.clone(), active_note, arg: self.view_args.remove(&v.id) };
        (v.render)(&ctx, window, cx)
    }

    /// Plugin views on the right; a row of icons picks the view.
    fn render_right_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let views = Self::views(cx, Side::Right);
        if !self.right_sidebar || views.is_empty() {
            return None;
        }
        let ix = self.right_view.as_ref().and_then(|id| views.iter().position(|v| &v.id == id)).unwrap_or(0);
        let theme = cx.theme().clone();
        let body = self.render_view(&views[ix], window, cx);
        Some(
            v_flex()
                .w(crate::ui::r(crate::ui::SIDEBAR))
                .h_full()
                .flex_none()
                .border_l_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .child(h_flex().gap_1().px_2().py_1().border_b_1().border_color(theme.border).children(views.iter().enumerate().map(|(i, v)| {
                    let id = v.id.clone();
                    crate::ui::icon_button(("right-view", i), v.icon).selected(i == ix).tooltip(v.title.clone()).on_click(cx.listener(move |ws, _, _, cx| {
                        ws.right_view = Some(id.clone());
                        ws.save_layout();
                        cx.notify();
                    }))
                })))
                .child(div().px_3().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(views[ix].title.to_uppercase()))
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


    pub(crate) fn toggle_sidebar(&mut self, left: bool, cx: &mut Context<Self>) {
        if left {
            self.sidebar = !self.sidebar;
        } else {
            self.right_sidebar = !self.right_sidebar;
        }
        self.save_layout();
        cx.notify();
    }

    /// Focus mode: only the note you're on (no ribbon, sidebars, tab bars, other panes or
    /// status bar; the editing toolbar stays so you can leave). `fullscreen` also puts the
    /// window in full screen. Toggling again restores everything as it was.
    pub(crate) fn toggle_focus_mode(&mut self, fullscreen: bool, window: &mut Window, cx: &mut Context<Self>) {
        match self.focus_mode.take() {
            Some((l, r, fs)) => {
                self.sidebar = l;
                self.right_sidebar = r;
                if fs && window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
            }
            None => {
                let fs = fullscreen && !window.is_fullscreen();
                if fs {
                    window.toggle_fullscreen();
                }
                self.focus_mode = Some((self.sidebar, self.right_sidebar, fs));
                self.sidebar = false;
                self.right_sidebar = false;
            }
        }
        if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
            println!("FOCUS {} fullscreen={}", self.focus_mode.is_some(), self.focus_mode.is_some_and(|f| f.2));
        }
        cx.notify();
    }


    pub(crate) fn reveal_active(&mut self, cx: &mut Context<Self>) {
        self.sidebar = true;
        self.left_view = None;
        self.save_layout();
        self.select_active_in_tree(cx);
        cx.notify();
    }

    fn active_rel(&self, cx: &App) -> Option<String> {
        self.active_editor().map(|e| self.rel(&e.read(cx).path))
    }

    pub(crate) fn rename_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rel) = self.active_rel(cx) {
            self.rename_dialog(rel, false, window, cx);
        }
    }

    pub(crate) fn delete_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rel) = self.active_rel(cx) {
            self.delete_dialog(rel, false, window, cx);
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
        let rel = match self.tabs.get(self.active) {
            Some(Tab::Note(e)) => self.rel(&e.read(cx).path),
            Some(Tab::View(v)) if v.kind == crate::image_view::VIEW => match &v.arg {
                Some(a) => a.clone(),
                None => return,
            },
            _ => return,
        };
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
        self.close_preview();
        if let Some(ix) = self.tabs.iter().position(|t| t.editor().is_some_and(|e| e.read(cx).path == path)) {
            self.activate(ix, window, cx);
            if let Some(e) = self.tabs[ix].editor() {
                return e.clone();
            }
        }
        let vault = self.vault.clone();
        let editor = cx.new(|cx| Editor::open(path, vault, window, cx));
        self._subs.push(cx.subscribe_in(&editor, window, Self::on_editor_event));
        self._subs.push(cx.observe(&editor, Self::sync_panes));
        // the note may already be open, with unsaved edits, in another pane: start from those
        if let Some(peer) = self.all_editors().into_iter().find(|o| *o != editor && o.read(cx).path == editor.read(cx).path) {
            let text = peer.read(cx).doc.text();
            if let Some((r, t)) = obsidilight_core::Document::diff(&editor.read(cx).doc.text(), &text) {
                editor.update(cx, |e, cx| e.apply_remote(r, &t, cx));
            }
            let v = editor.read(cx).doc.version();
            self.synced.insert(editor.entity_id(), v);
        }
        if new_tab || self.tabs.is_empty() {
            self.tabs.push(Tab::Note(editor.clone()));
            self.active = self.tabs.len() - 1;
        } else {
            // leaving the replaced note: make sure its edits are on disk
            let old = self.tabs[self.active].clone();
            old.save(cx);
            self.tabs[self.active] = Tab::Note(editor.clone());
        }
        let f = editor.read(cx).focus.clone();
        window.focus(&f, cx);
        self.select_active_in_tree(cx);
        self.note_recent(cx);
        cx.notify();
        editor
    }

    /// Opens any vault file: images in an image tab, everything else in an editor (Markdown
    /// for notes, plain text for other files). Returns the editor, if one was opened.
    pub fn open_file(&mut self, path: PathBuf, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Editor>> {
        if !crate::embeds::is_image(&path) {
            return Some(self.open_path(path, new_tab, window, cx));
        }
        let rel = self.rel(&path);
        let arg = Some(rel);
        if let Some(ix) = self.tabs.iter().position(|t| matches!(t, Tab::View(v) if v.kind == crate::image_view::VIEW && v.arg == arg)) {
            self.activate(ix, window, cx);
            return None;
        }
        // like a note: replace the active tab unless a new one was asked for
        let replace = !new_tab && !self.tabs.is_empty();
        let old = replace.then(|| self.tabs[self.active].clone());
        if let Some(t) = &old {
            t.save(cx);
        }
        let at = self.active;
        self.open_view(crate::image_view::VIEW, arg, window, cx);
        if replace {
            let new = self.tabs.pop().expect("open_view added a tab");
            self.active = at;
            self.tabs[at] = new;
        }
        self.select_active_in_tree(cx);
        cx.notify();
        None
    }

    /// Opens a vault file by vault path, placing the caret.
    pub fn open_rel(&mut self, rel: &str, at: OpenAt, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.vault.borrow().root.join(rel);
        let Some(ed) = self.open_file(path, new_tab, window, cx) else { return };
        match at {
            OpenAt::Keep => {}
            OpenAt::Offset(off) => ed.update(cx, |e, cx| {
                let off = e.doc.clamp(off);
                e.set_selection(obsidilight_core::Selection::caret(off), false, cx);
            }),
            OpenAt::Subpath(sub) => ed.update(cx, |e, cx| e.go_to_subpath(&sub, cx)),
        }
    }

    /// Creates a note (vault path; `.md` added; folders created) and opens it.
    pub fn create_and_open(&mut self, rel: &str, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        self.create_with_text(rel, "", new_tab, window, cx)
    }

    /// Like [`create_and_open`](Self::create_and_open), writing `text` into a note that doesn't
    /// exist yet (an existing note is opened unchanged).
    pub fn create_with_text(&mut self, rel: &str, text: &str, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let mut rel = rel.trim().trim_matches('/').to_string();
        if !is_note(&rel) {
            rel.push_str(".md");
        }
        if rel.split('/').any(|c| c.is_empty() || c == ".." || c.starts_with('.')) {
            return Err("invalid name".into());
        }
        let path = self.vault.borrow().root.join(&rel);
        if !path.exists() {
            if let Some(d) = path.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            {
                use std::io::Write as _;
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
                f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            }
            self.vault.borrow_mut().index.refresh(&rel);
            self.rebuild_tree(cx);
            for t in self.all_editors() {
                t.update(cx, |e, cx| e.invalidate(cx));
            }
        }
        self.open_path(path, new_tab, window, cx);
        Ok(())
    }

    /// Opens a plugin's tab view in the focused pane (or focuses it, if already there).
    pub fn open_view(&mut self, id: &str, arg: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview();
        if let Some(ix) = self.tabs.iter().position(|t| matches!(t, Tab::View(v) if v.kind == id && v.arg == arg)) {
            self.activate(ix, window, cx);
            return;
        }
        let Some(t) = cx.try_global::<PluginHost>().and_then(|h| h.reg.tab_views.iter().find(|(_, t)| t.id == id).map(|(_, t)| t.clone())) else { return };
        let (view, focus) = (t.create)(arg.clone(), window, cx);
        let title = match (&arg, t.title_from_arg) {
            (Some(a), true) => a.rsplit('/').next().unwrap_or(a).to_string(),
            _ => t.title.clone(),
        };
        self.tabs.push(Tab::View(ViewTab { kind: id.to_string(), arg, title: title.into(), view, focus: focus.clone() }));
        self.active = self.tabs.len() - 1;
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Tabs of views whose plugin was turned off go away.
    fn prune_view_tabs(&mut self, cx: &App) {
        let kinds: HashSet<String> = cx.try_global::<PluginHost>().map(|h| h.reg.tab_views.iter().map(|(_, t)| t.id.clone()).collect()).unwrap_or_default();
        let keep = |t: &Tab| !matches!(t, Tab::View(v) if !kinds.contains(&v.kind));
        if self.tabs.iter().all(keep) && self.parked.values().all(|(ts, _)| ts.iter().all(keep)) {
            return;
        }
        self.tabs.retain(keep);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        for (ts, a) in self.parked.values_mut() {
            ts.retain(keep);
            *a = (*a).min(ts.len().saturating_sub(1));
        }
    }

    /// Reveals a sidebar view (opening its sidebar) and hands it an argument.
    pub fn show_view(&mut self, id: &str, arg: Option<String>, cx: &mut Context<Self>) {
        let Some(v) = Self::views(cx, Side::Left).into_iter().chain(Self::views(cx, Side::Right)).find(|v| v.id == id) else { return };
        if let Some(a) = arg {
            self.view_args.insert(id.to_string(), a);
        }
        match v.side {
            Side::Left => {
                self.sidebar = true;
                self.left_view = Some(id.to_string());
            }
            Side::Right => {
                self.right_sidebar = true;
                self.right_view = Some(id.to_string());
            }
        }
        self.save_layout();
        cx.notify();
    }

    fn note_recent(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.active_editor() else { return };
        let rel = self.rel(&e.read(cx).path);
        self.recent.retain(|r| *r != rel);
        self.recent.insert(0, rel);
        self.recent.truncate(50);
        self.save_layout();
    }

    // ---- layout (`.obsidilight/workspace.json`) --------------------------------------

    fn layout_path(&self) -> PathBuf {
        self.vault.borrow().root.join(".obsidilight/workspace.json")
    }

    fn load_layout(&mut self) {
        let Some(v) = std::fs::read_to_string(self.layout_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
        self.left_view = s("leftView");
        self.right_view = s("rightView");
        self.sidebar = v.get("leftSidebar").and_then(|x| x.as_bool()).unwrap_or(true);
        self.right_sidebar = v.get("rightSidebar").and_then(|x| x.as_bool()).unwrap_or(false);
        self.recent = v.get("recent").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    }

    fn save_layout(&self) {
        // benchmarks and UI tests run on throwaway copies, but never write while benchmarking
        if std::env::var_os("OBSIDILIGHT_BENCH").is_some() {
            return;
        }
        let v = serde_json::json!({
            "leftSidebar": self.sidebar, "leftView": self.left_view,
            "rightSidebar": self.right_sidebar, "rightView": self.right_view,
            "recent": self.recent,
        });
        let p = self.layout_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.obsidilight-tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }

    pub(crate) fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview();
        if ix < self.tabs.len() {
            self.active = ix;
            let f = self.tabs[ix].focus(cx);
            window.focus(&f, cx);
            self.select_active_in_tree(cx);
            self.note_recent(cx);
            cx.notify();
        }
    }

    pub(crate) fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        let t = self.tabs[ix].clone();
        t.save(cx);
        self.tabs.remove(ix);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        if self.tabs.is_empty() {
            if self.pane_ids().len() > 1 {
                self.close_pane(window, cx); // an emptied split pane goes away
            } else {
                window.focus(&self.focus, cx);
            }
        } else {
            let a = self.active;
            self.activate(a, window, cx);
        }
        cx.notify();
    }

    /// Shows (after a short delay) or hides the hover preview of a vault note for a sidebar row.
    pub fn preview_link(&mut self, target: Option<(String, String)>, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((rel, sub)) = target else {
            self.preview_pending = None;
            if self.preview.take().is_some() {
                cx.notify();
            }
            return;
        };
        let key = format!("{rel}#{sub}");
        if self.preview.as_ref().is_some_and(|p| p.key == key) || self.preview_pending.as_ref() == Some(&key) {
            return;
        }
        if self.preview.take().is_some() {
            cx.notify();
        }
        self.preview_pending = Some(key.clone());
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let _ = ws.update(cx, |ws, cx| {
                if ws.preview_pending.as_ref() != Some(&key) {
                    return;
                }
                ws.preview_pending = None;
                let Some(text) = ws.note_text(&rel, cx) else { return };
                let Some((snippet, first)) = crate::editor::preview::snippet(&text, &sub) else { return };
                let numbers = crate::editor::preview::numbers(&rel, &text, first, &ws.vault.borrow().index, cx);
                if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
                    println!("PREVIEW {key} {:?}", snippet.lines().next().unwrap_or(""));
                }
                ws.preview = Some(WsPreview { key, rel, at, text: text.into(), snippet, numbers });
                cx.notify();
            });
        })
        .detach();
    }

    /// Opening something ends any hover preview (its row or node may be gone now).
    fn close_preview(&mut self) {
        self.preview = None;
        self.preview_pending = None;
    }

    fn render_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.preview.as_ref()?;
        let vault = self.vault.clone();
        let path = vault.borrow().root.join(&p.rel);
        let md = crate::editor::render::MarkdownRenderer::new(Rc::new(move |t: &str| vault.borrow().resolve(t, &path))).with_note(p.rel.clone(), p.text.clone());
        let (snippet, numbers, at) = (p.snippet.clone(), p.numbers.clone(), p.at);
        Some(crate::editor::preview::card(&md, &snippet, numbers, at, window, cx))
    }

    /// Live sync: when a note's text changed in one pane, every other pane showing that note
    /// gets the difference at once (every keystroke, not only on save).
    fn sync_panes(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) {
        let (id, version, path) = {
            let e = editor.read(cx);
            (editor.entity_id(), e.doc.version(), e.path.clone())
        };
        if self.synced.get(&id) == Some(&version) {
            return;
        }
        self.synced.insert(id, version);
        let peers: Vec<Entity<Editor>> = self.all_editors().into_iter().filter(|o| *o != editor && o.read(cx).path == path).collect();
        if peers.is_empty() {
            return;
        }
        let text = editor.read(cx).doc.text();
        for p in peers {
            let Some((range, new)) = obsidilight_core::Document::diff(&p.read(cx).doc.text(), &text) else { continue };
            p.update(cx, |e, cx| e.apply_remote(range, &new, cx));
            let v = p.read(cx).doc.version();
            self.synced.insert(p.entity_id(), v);
        }
    }

    fn on_editor_event(&mut self, editor: &Entity<Editor>, ev: &EditorEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            EditorEvent::Saved(p) => {
                let rel = self.rel(p);
                let old = self.vault.borrow().index.text(&rel).map(String::from);
                self.vault.borrow_mut().index.refresh(&rel);
                let new = editor.read(cx).doc.text();
                for l in Self::listeners(cx) {
                    l.modified(&rel, old.as_deref(), &new, false, cx);
                }
                // the same note open in another pane: show what was just saved
                for other in self.all_editors() {
                    if other != *editor && other.read(cx).path == *p {
                        other.update(cx, |e, cx| e.on_disk_changed(cx));
                    }
                }
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
                            for t in self.all_editors() {
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
                let ed = if path == from {
                    editor.clone()
                } else {
                    match self.open_file(path, *new_tab, window, cx) {
                        Some(e) => e,
                        None => return, // an image
                    }
                };
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
    pub(crate) fn new_note_in(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Some(e) = self.active_editor() {
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
        for e in &self.all_editors() {
            e.update(cx, |e, cx| e.save(cx));
        }
        if self.all_editors().iter().any(|e| e.read(cx).doc.is_dirty()) {
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
        for e in &self.all_editors() {
            let p = e.read(cx).path.clone();
            let rel = self.rel(&p);
            if let Some((_, n)) = moves.iter().find(|(o, _)| *o == rel) {
                let np = root.join(n);
                e.update(cx, |e, cx| e.set_path(np, cx));
            }
        }
        // image tabs follow their files
        let root2 = root.clone();
        let retarget = |t: &mut Tab, cx: &mut Context<Self>| {
            let Tab::View(v) = t else { return };
            if v.kind != crate::image_view::VIEW {
                return;
            }
            let Some((_, n)) = v.arg.as_ref().and_then(|a| moves.iter().find(|(o, _)| o == a)) else { return };
            let view = cx.new(|cx| crate::image_view::ImageView::new(root2.join(n), cx));
            v.focus = view.read(cx).focus_handle(cx);
            v.view = view.into();
            v.title = n.rsplit('/').next().unwrap_or(n).to_string().into();
            v.arg = Some(n.clone());
        };
        let mut tabs = std::mem::take(&mut self.tabs);
        tabs.iter_mut().for_each(|t| retarget(t, cx));
        self.tabs = tabs;
        let mut parked = std::mem::take(&mut self.parked);
        for (ts, _) in parked.values_mut() {
            ts.iter_mut().for_each(|t| retarget(t, cx));
        }
        self.parked = parked;
        // 4. rewrite links: through the editor if the note is open, else straight to disk
        let (links, failures) = self.apply_note_edits(&plan, cx);
        // 5. re-index, tell plugins (bookmarks), keep the recent list pointing at the files
        let fresh = obsidilight_core::index::VaultIndex::build(&root);
        self.vault.borrow_mut().index = fresh;
        for r in self.recent.iter_mut() {
            if let Some((_, n)) = moves.iter().find(|(o, _)| o == r) {
                *r = n.clone();
            }
        }
        self.save_layout();
        let listeners: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.vault_listeners.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default();
        for l in listeners {
            l.renamed(old, &new, &moves, cx);
        }
        if old_abs.is_dir() || new_abs.is_dir() {
            self.expanded.insert(format!("dir:{new}"));
        }
        self.rebuild_tree(cx);
        for e in &self.all_editors() {
            e.update(cx, |e, cx| e.invalidate(cx));
        }
        cx.notify();
        if failures.is_empty() {
            Ok((links, plan.len()))
        } else {
            Err(format!("moved, but links in {} note(s) could not be updated: {}", failures.len(), failures.join(", ")))
        }
    }

    fn listeners(cx: &App) -> Vec<Rc<dyn crate::plugins::VaultListener>> {
        cx.try_global::<PluginHost>().map(|h| h.reg.vault_listeners.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default()
    }

    /// Applies edits to notes: through the editor when the note is open (undoable, saved at
    /// once), else on disk with the usual "never overwrite a newer file" check. Returns the
    /// number of edits and the notes that could not be written.
    fn apply_note_edits(&mut self, plan: &[obsidilight_core::index::NoteEdit], cx: &mut Context<Self>) -> (usize, Vec<String>) {
        let root = self.vault.borrow().root.clone();
        let mut n = 0;
        let mut failures = Vec::new();
        for edit in plan {
            n += edit.edits.len();
            let path = root.join(&edit.note);
            if let Some(e) = self.all_editors().iter().find(|e| e.read(cx).path == path) {
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
        (n, failures)
    }

    /// The current text of a note: the open editor's (unsaved edits included), else the disk's.
    pub fn note_text(&self, rel: &str, cx: &App) -> Option<String> {
        let path = self.vault.borrow().root.join(rel);
        match self.all_editors().iter().find(|e| e.read(cx).path == path) {
            Some(e) => Some(e.read(cx).doc.text()),
            None => std::fs::read_to_string(path).ok(),
        }
    }

    /// Replaces a note's whole text (through its editor if open). The note must exist.
    pub fn set_note_text(&mut self, rel: &str, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let path = self.vault.borrow().root.join(rel);
        if let Some(e) = self.all_editors().into_iter().find(|e| e.read(cx).path == path) {
            let len = e.read(cx).doc.len();
            e.update(cx, |e, cx| e.apply_edits(&[(0..len, text.to_string())], cx));
            return if e.read(cx).doc.is_dirty() { Err(format!("\"{rel}\" could not be saved")) } else { Ok(()) };
        }
        let l = obsidilight_core::file::load(&path).map_err(|e| e.to_string())?;
        match obsidilight_core::file::save(&path, text, Some(l.disk)) {
            Ok(obsidilight_core::file::SaveOutcome::Saved(_) | obsidilight_core::file::SaveOutcome::Unchanged(_)) => {
                self.vault.borrow_mut().index.refresh(rel);
                Ok(())
            }
            Ok(_) => Err(format!("\"{rel}\" changed on disk meanwhile")),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Creates a note with `text` without opening it; returns its vault path (`.md` added).
    pub fn create_file(&mut self, rel: &str, text: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let mut rel = rel.trim().trim_matches('/').to_string();
        if !is_note(&rel) {
            rel.push_str(".md");
        }
        if rel.split('/').any(|c| c.is_empty() || c == ".." || c.starts_with('.')) {
            return Err("invalid name".into());
        }
        let path = self.vault.borrow().root.join(&rel);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
            f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        self.vault.borrow_mut().index.refresh(&rel);
        self.rebuild_tree(cx);
        for t in self.all_editors() {
            t.update(cx, |e, cx| e.invalidate(cx));
        }
        Ok(rel)
    }

    /// Merges note `src` into `dst` (note composer): the source text (its own links updated,
    /// then passed through `wrap`, e.g. a template) is added at the end of `dst` (or the
    /// start), every link to `src` is pointed at `dst`, and `src` goes to the trash. Returns the number of links updated.
    pub fn merge_notes(&mut self, src: &str, dst: &str, wrap: &dyn Fn(&str) -> String, prepend: bool, window: &mut Window, cx: &mut Context<Self>) -> Result<usize, String> {
        if src == dst {
            return Err("a note cannot be merged into itself".into());
        }
        for e in &self.all_editors() {
            e.update(cx, |e, cx| e.save(cx));
        }
        if self.all_editors().iter().any(|e| e.read(cx).doc.is_dirty()) {
            return Err("a note has unsaved changes that could not be saved".into());
        }
        let plan = self.vault.borrow().index.merge_plan(src, dst);
        // links inside the source move with its text: rewrite them in the text being merged
        let (src_plan, rest): (Vec<_>, Vec<_>) = plan.into_iter().partition(|e| e.note == src);
        let text = self.note_text(src, cx).ok_or_else(|| format!("cannot read \"{src}\""))?;
        let content = wrap(&match src_plan.first() {
            Some(e) => obsidilight_core::index::apply_edits(&text, &e.edits),
            None => text,
        });
        let (links, failures) = self.apply_note_edits(&rest, cx);
        if !failures.is_empty() {
            return Err(format!("links in {} could not be updated; nothing was merged", failures.join(", ")));
        }
        let old = self.note_text(dst, cx).ok_or_else(|| format!("cannot read \"{dst}\""))?;
        let joined = |a: &str, b: &str| if a.is_empty() { b.to_string() } else if b.is_empty() { a.to_string() } else { format!("{}\n\n{}", a.trim_end_matches(['\n', '\r']), b) };
        let new = if prepend { joined(&content, &old) } else { joined(&old, &content) };
        self.set_note_text(dst, &new, cx)?;
        self.delete_path(src, window, cx)?;
        let root = self.vault.borrow().root.clone();
        self.open_path(root.join(dst), false, window, cx);
        Ok(links + src_plan.first().map_or(0, |e| e.edits.len()))
    }

    /// Moves a file or folder to the system trash (Obsidian's default), closing its tabs.
    pub fn delete_path(&mut self, rel: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let root = self.vault.borrow().root.clone();
        let abs = root.join(rel);
        // tabs of it in other panes go first (saving disabled: the file is going away)
        for (tabs, active) in self.parked.values_mut() {
            tabs.retain(|t| {
                let Some(e) = t.editor() else { return true };
                let gone = e.read(cx).path.starts_with(&abs);
                if gone {
                    e.update(cx, |e, _| e.disable_saving());
                }
                !gone
            });
            *active = (*active).min(tabs.len().saturating_sub(1));
        }
        let doomed: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.editor().is_some_and(|e| e.read(cx).path.starts_with(&abs))).map(|(i, _)| i).collect();
        for &i in doomed.iter().rev() {
            if let Some(e) = self.tabs[i].editor() {
                e.update(cx, |e, _| e.disable_saving());
            }
            self.close_tab(i, window, cx);
        }
        // image tabs of what is going away close
        let gone = |t: &Tab| matches!(t, Tab::View(v) if v.kind == crate::image_view::VIEW && v.arg.as_ref().is_some_and(|a| a == rel || a.starts_with(&format!("{rel}/"))));
        for (tabs, active) in self.parked.values_mut() {
            tabs.retain(|t| !gone(t));
            *active = (*active).min(tabs.len().saturating_sub(1));
        }
        while let Some(i) = self.tabs.iter().position(gone) {
            self.close_tab(i, window, cx);
        }
        trash::delete(&abs).map_err(|e| e.to_string())?;
        let fresh = obsidilight_core::index::VaultIndex::build(&root);
        self.vault.borrow_mut().index = fresh;
        let prefix = format!("{rel}/");
        self.recent.retain(|r| r != rel && !r.starts_with(&prefix));
        self.save_layout();
        let listeners: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.vault_listeners.iter().map(|(_, l)| l.clone()).collect()).unwrap_or_default();
        for l in listeners {
            l.deleted(rel, cx);
        }
        self.rebuild_tree(cx);
        for e in &self.all_editors() {
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

    fn watch(&self, root: &Path, cx: &mut Context<Self>) -> Option<Rc<crate::watcher::VaultWatcher>> {
        let w = Rc::new(crate::watcher::VaultWatcher::new(root).ok()?);
        let queue = w.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let changed = queue.drain();
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
        let old: Vec<(String, Option<String>)> = rels.iter().filter(|r| is_note(r)).map(|r| (r.clone(), self.vault.borrow().index.text(r).map(String::from))).collect();
        self.vault.borrow_mut().index.refresh_batch(rels);
        let listeners = Self::listeners(cx);
        if !listeners.is_empty() {
            for (rel, before) in &old {
                let now = self.vault.borrow().index.text(rel).map(String::from);
                if let Some(now) = now.filter(|n| Some(n) != before.as_ref()) {
                    for l in &listeners {
                        l.modified(rel, before.as_deref(), &now, true, cx);
                    }
                }
            }
        }
        let root = self.vault.borrow().root.clone();
        for e in &self.all_editors() {
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
        for e in &self.all_editors() {
            e.update(cx, |e, cx| e.invalidate(cx));
        }
        cx.notify();
        if log {
            eprintln!("TIMING files_changed {} paths ({:?}…) took {:.1} ms", rels.len(), rels.first(), t.elapsed().as_secs_f64() * 1000.0);
        }
    }

    // ---- rendering ------------------------------------------------------------------

    /// The left sidebar: a row of view icons (Files, then plugin views such as Search and
    /// Bookmarks) over the chosen view.
    fn render_left(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let views = Self::views(cx, Side::Left);
        let current = self.left_view.as_ref().and_then(|id| views.iter().position(|v| &v.id == id));
        let theme = cx.theme().clone();
        let mut tabs = vec![crate::ui::icon_button("left-files", IconName::Folder).selected(current.is_none()).tooltip("Files").on_click(cx.listener(|ws, _, _, cx| {
            ws.left_view = None;
            ws.save_layout();
            cx.notify();
        }))];
        for (i, v) in views.iter().enumerate() {
            let id = v.id.clone();
            tabs.push(crate::ui::icon_button(("left-view", i), v.icon).selected(current == Some(i)).tooltip(v.title.clone()).on_click(cx.listener(move |ws, _, _, cx| {
                ws.left_view = Some(id.clone());
                ws.save_layout();
                cx.notify();
            })));
        }
        let header = h_flex().gap_1().px_2().py_1().border_b_1().border_color(theme.border).children(tabs);
        let body = match current {
            None => self.render_sidebar(cx).into_any_element(),
            Some(i) => {
                let v = views[i].clone();
                v_flex()
                    .size_full()
                    .child(div().px_3().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(v.title.to_uppercase()))
                    .child(div().flex_1().min_h_0().overflow_hidden().p_2().child(self.render_view(&v, window, cx)))
                    .into_any_element()
            }
        };
        v_flex().w(crate::ui::r(crate::ui::SIDEBAR)).h_full().flex_none().border_r_1().border_color(theme.border).bg(theme.sidebar).child(header).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let root = self.vault.borrow().root.clone();
        let target = self.menu_target.clone();
        let menu_ws = cx.entity().downgrade();
        v_flex()
            .size_full()
            .child(h_flex().px_3().py_2().justify_between().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Files")).child(
                crate::ui::icon_button("new-note", IconName::Plus).tooltip("New note (Ctrl+N)").on_click(cx.listener(|ws, _, w, cx| ws.new_note_in("", w, cx))),
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
                        let row = h_flex()
                            .gap_1p5()
                            .text_size(crate::ui::r(13.))
                            .text_color(if conflict { warn } else { fg })
                            .child(Icon::new(icon).small())
                            .child(item.label.clone())
                            .when(conflict, |d| d.child(div().text_xs().child("sync conflict")));
                        // notes preview on (Ctrl+)hover, like Obsidian's file explorer
                        let row = if !folder && is_note(&id) {
                            use crate::plugins::page_preview::PreviewOnHover as _;
                            div().id(SharedString::from(format!("tree-row:{id}"))).w_full().child(row).preview_on_hover(&WorkspaceHandle(weak.clone()), &id, "").into_any_element()
                        } else {
                            row.into_any_element()
                        };
                        ListItem::new(ix)
                            .w_full()
                            .h(crate::ui::r(25.))
                            .py_0()
                            .px_2()
                            .pl(crate::ui::r(17. * entry.depth() as f32 + 8.))
                            .child(row)
                            .when(!folder, |li| {
                                li.on_click(move |ev, window, cx| {
                                    let new_tab = ev.modifiers().secondary(cx);
                                    let p = path.clone();
                                    let _ = weak.update(cx, |ws, cx| {
                                        ws.open_file(p, new_tab, window, cx);
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

impl Workspace {
    fn render_layout(&mut self, layout: &Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match layout {
            Layout::Pane(id) => self.render_pane(*id, window, cx),
            Layout::Split { id, axis, children } => {
                let group = match axis {
                    Axis::Horizontal => gpui_kit::component::resizable::h_resizable(("split", *id)),
                    Axis::Vertical => gpui_kit::component::resizable::v_resizable(("split", *id)),
                };
                let kids: Vec<AnyElement> = children.iter().map(|c| self.render_layout(c, window, cx)).collect();
                group.children(kids.into_iter().map(|k| gpui_kit::component::resizable::resizable_panel().child(k))).into_any_element()
            }
        }
    }

    /// One pane: its tab bar and active editor. A click anywhere in it focuses it.
    fn render_pane(&mut self, id: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let focused = id == self.pane;
        let (tabs, active) = if focused { (self.tabs.clone(), self.active) } else { self.parked.get(&id).cloned().unwrap_or_default() };
        let split = self.pane_ids().len() > 1 && self.focus_mode.is_none();
        let names: Vec<(SharedString, bool)> = tabs.iter().map(|t| t.label(cx)).collect();
        let current = tabs.get(active).cloned();
        let editor = current.as_ref().and_then(|t| t.editor().cloned());
        v_flex()
            .id(("pane", id))
            .size_full()
            .min_w_0()
            .capture_any_mouse_down(cx.listener(move |ws, _, w, cx| ws.focus_pane(id, w, cx)))
            .when(split && focused, |d| d.border_t_2().border_color(theme.primary.opacity(0.6)))
            .when(split && !focused, |d| d.border_t_2().border_color(theme.border))
            .when(!names.is_empty() && self.focus_mode.is_none(), |d| {
                d.child(
                    TabBar::new(("tabs", id))
                        .suffix(
                            crate::ui::icon_button(SharedString::from(format!("split-{id}")), IconName::PanelRight).tooltip("Split right").on_click(cx.listener(move |ws, ev: &ClickEvent, w, cx| {
                                ws.focus_pane(id, w, cx);
                                // Shift+click splits down
                                ws.split(if ev.modifiers().shift { Axis::Vertical } else { Axis::Horizontal }, w, cx)
                            })),
                        )
                        .selected_index(active)
                        .on_click(cx.listener(move |ws, ix: &usize, w, cx| {
                            ws.focus_pane(id, w, cx);
                            ws.activate(*ix, w, cx)
                        }))
                        .children(names.into_iter().enumerate().map(|(ix, (name, dirty))| {
                            TabButton::new().label(if dirty { format!("{name} •").into() } else { name }).suffix(
                                Button::new(SharedString::from(format!("close-tab-{id}-{ix}"))).ghost().xsmall().icon(IconName::Close).on_click(cx.listener(move |ws, _, w, cx| {
                                    ws.focus_pane(id, w, cx);
                                    ws.close_tab(ix, w, cx)
                                })),
                            )
                        })),
                )
            })
            .when(focused && editor.as_ref().is_some_and(|e| !e.read(cx).plain), |d| {
                let slots: Vec<_> = cx.try_global::<PluginHost>().map(|h| h.reg.toolbars.iter().map(|(_, t)| t.clone()).collect()).unwrap_or_default();
                let ctx = ViewCtx { editor: editor.clone(), workspace: WorkspaceHandle(cx.entity().downgrade()), vault: self.vault.clone(), active_note: editor.as_ref().map(|e| self.rel(&e.read(cx).path)), arg: None };
                d.children(slots.iter().map(|t| (t.render)(&ctx, _window, cx)))
            })
            .child(match current {
                Some(Tab::Note(e)) => div().flex_1().min_h_0().child(e).into_any_element(),
                Some(Tab::View(v)) => div().flex_1().min_h_0().child(v.view).into_any_element(),
                // The message wraps to the pane's width and is clipped to it: as one unwrappable
                // line centred in a narrow pane it spilled over both sidebars.
                None => div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .px_4()
                    .child(div().w_full().text_center().text_color(theme.muted_foreground).child("No file open — pick one on the left, or Ctrl+N for a new note"))
                    .into_any_element(),
            })
            .into_any_element()
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
        self.prune_view_tabs(cx);
        // focus mode shows the focused pane alone
        let layout = if self.focus_mode.is_some() { Layout::Pane(self.pane) } else { self.layout.clone() };
        let panes = self.render_layout(&layout, window, cx);
        let sidebar = self.sidebar.then(|| self.render_left(window, cx));
        let focus = self.focus_mode.is_some();
        let ribbon = (!focus).then(|| self.render_ribbon(cx).into_any_element());
        let right = self.render_right_sidebar(window, cx);
        let status = if focus { None } else { self.render_status_bar(cx) };
        let preview = self.render_preview(window, cx);
        div()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            // Ctrl pressed or released over a link: replay the mouse position so the hover
            // handlers (editor, sidebars, graph) see the new keys and show or hide previews
            .on_modifiers_changed(|ev: &ModifiersChangedEvent, window, cx| {
                let (position, modifiers) = (window.mouse_position(), ev.modifiers);
                window.defer(cx, move |window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                });
            })
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
            .on_action(cx.listener(|_, _: &OpenSettings, w, cx| crate::settings_ui::open(w, cx)))
            .on_action(cx.listener(|ws, _: &ToggleRightSidebar, _, cx| {
                ws.right_sidebar = !ws.right_sidebar;
                ws.save_layout();
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ToggleSidebar, _, cx| {
                ws.sidebar = !ws.sidebar;
                ws.save_layout();
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
                    ws.open_file(path, true, w, cx);
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
            .children(ribbon)
            .children(sidebar)
            .child(v_flex().flex_1().min_w_0().h_full().child(div().flex_1().min_h_0().flex().child(panes)).children(status))
            .children(right)
            .children(preview)
    }
}
