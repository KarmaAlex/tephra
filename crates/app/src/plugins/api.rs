//! The plugin API (HANDOFF §5.2, decisions D5/D7). Every bundled feature — core plugins and
//! replacements for Obsidian community plugins — is built on exactly this API; nothing gets
//! private hooks. Plugins are first-party and in-process; the shape is meant to allow
//! third-party plugins later (sandboxing is out of scope).

#![allow(dead_code)] // API surface for plugins; not every entry point has a user yet

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::*;
use serde_json::Value;

use crate::editor::{Editor, EditorApi};
pub use crate::editor::render::MarkdownRenderer;

pub type PluginId = &'static str;

// ---- manifest & settings schema -------------------------------------------------------

#[derive(Clone, Debug)]
pub enum SettingKind {
    Toggle,
    Text,
    Number,
    /// (value, label) pairs.
    Choice(Vec<(&'static str, &'static str)>),
    /// Stored and passed to the plugin, not shown (imported advanced values).
    Hidden,
}

#[derive(Clone, Debug)]
pub struct SettingSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: SettingKind,
    pub default: Value,
}

#[derive(Clone, Debug)]
pub struct PluginManifest {
    pub id: PluginId,
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub enabled_by_default: bool,
    /// Drives the generated settings tab; values live in `.obsidilight/plugins/<id>.json`.
    pub settings: Vec<SettingSpec>,
    /// Code-block languages this plugin renders; while it is off, the core shows those blocks
    /// as code with a "<name> plugin disabled" note.
    pub code_languages: &'static [&'static str],
}

/// A plugin. `on_load` registers everything through the [`Registrar`]; whatever it registered
/// is removed automatically on unload (toggling applies without a restart). Loading has no
/// access to the app: anything that needs it runs from a registered callback.
pub trait Plugin: 'static {
    fn manifest(&self) -> &PluginManifest;
    /// `settings` holds this plugin's current values (defaults filled in).
    fn on_load(&mut self, settings: &serde_json::Map<String, Value>, reg: &mut Registrar);
    /// Extra cleanup beyond registrations (which the host removes itself).
    fn on_unload(&mut self) {}
}

// ---- registration points ----------------------------------------------------------------

/// The overall look of a callout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutLook {
    /// Obsidian's default: tinted box, icon + coloured title.
    Obsidian,
    /// Bordered box with the label as a filled badge (Academic Notes theorem-likes).
    Badge,
    /// A float: the body centred with a caption line (bold label, then the title), below the
    /// body (figures) or above it (tables).
    Caption { below: bool },
    /// No box: accent bar and an italic run-in label (proofs, remarks).
    Soft,
    /// Rules above and below (algorithms).
    Rules,
}

/// How a callout type looks (D7). The core supplies Obsidian's defaults; a plugin may claim types.
#[derive(Clone, Debug, PartialEq)]
pub struct CalloutStyle {
    pub color: Hsla,
    pub look: CalloutLook,
    /// Header icon (Obsidian look).
    pub icon: Option<IconName>,
    /// Text shown for the type (e.g. "Theorem") next to the title. None = Obsidian's rule:
    /// the title alone, or the capitalised type when there is no title.
    pub label: Option<String>,
}

/// A note as plugins see it while numbering or labelling: its text (with unsaved edits, for
/// the note being edited), its vault path, and the read-only vault index (for things that
/// depend on other notes, e.g. which equations are referenced).
pub struct NoteCtx<'a> {
    pub path: &'a str,
    pub text: &'a str,
    pub index: &'a obsidilight_core::index::VaultIndex,
}

pub trait CalloutRenderer {
    /// Style for `kind` (lowercased) with `|meta`, or None to leave it to others / the core.
    fn style(&self, kind: &str, meta: &str, dark: bool) -> Option<CalloutStyle>;
    /// Labels by line: a callout's header line ("Theorem 1.2"), or the first line of a display
    /// formula ("(1.2)", drawn at its right).
    fn numbers(&self, _note: &NoteCtx) -> std::collections::HashMap<usize, String> {
        Default::default()
    }
    /// Draws a callout's body itself (e.g. pseudocode); `body` = its lines without quote markers.
    fn render_body(&self, _kind: &str, _meta: &str, _body: &[String], _md: &MarkdownRenderer, _window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        None
    }
    /// The body as Typst for PDF export, when `render_body` draws it specially; `md` converts
    /// one line of Markdown to Typst.
    fn export_body(&self, _kind: &str, _meta: &str, _body: &[String], _md: &dyn Fn(&str) -> String) -> Option<String> {
        None
    }
    /// Something to show after the body (e.g. the end-of-proof mark).
    fn body_end(&self, _kind: &str) -> Option<String> {
        None
    }
}

/// Display text for internal links without an alias (references: `[[#^lemma1]]` → "lem 1.1").
pub trait LinkLabeler {
    /// `target` is the note the link points into; `sub` is the part after `#` (e.g. `^lemma1`).
    fn label(&self, target: &NoteCtx, sub: &str) -> Option<String>;
}

/// Renders fenced code blocks of one language (e.g. `chart`, `desmos-graph`) when the caret is
/// outside them; the source shows when the caret enters.
pub trait CodeBlockRenderer {
    /// `md` renders Markdown (read only) with links and embeds resolved from the note, for
    /// blocks whose content is itself Markdown (layouts, columns).
    fn render(&self, code: &str, md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> AnyElement;
    /// The block in exported documents (PDF is typeset with Typst, M5): Typst markup, using
    /// `md` to convert nested Markdown. None = exported as a plain code listing.
    fn export_typst(&self, _code: &str, _md: &dyn Fn(&str) -> String) -> Option<String> {
        None
    }
}

/// Keys an editor hook may intercept (typed characters go through `on_char`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKey {
    Tab,
    ShiftTab,
    Enter,
    ShiftEnter,
    Backspace,
}

/// Editor behaviour (the CM6-extension equivalent): runs before the editor's default handling.
/// Return true when the input was consumed.
pub trait EditorHook {
    fn on_char(&self, _e: &mut EditorApi, _ch: &str) -> bool {
        false
    }
    fn on_key(&self, _e: &mut EditorApi, _key: EditorKey) -> bool {
        false
    }
}

/// Marks ranges of the text (spell checking, linting). Called per block with the block's source
/// text; results are cached by the editor until the block's text or `revision()` changes.
pub trait TextDecorator {
    /// Ranges within `block` (byte offsets into it) to underline.
    fn decorate(&self, block: &str) -> Vec<Range<usize>>;
    /// Bumped when results may change for unchanged text (e.g. a word was added).
    fn revision(&self) -> u64 {
        0
    }
    /// False while still loading (the editor checks again shortly).
    fn ready(&self) -> bool {
        true
    }
    fn color(&self) -> Hsla {
        red()
    }
}

/// An entry of the editor's right-click menu computed when it opens.
#[derive(Clone)]
pub struct MenuEntry {
    pub label: String,
    pub run: Action,
}

/// Builds editor right-click entries from the editor's state (e.g. spelling suggestions for the
/// word under the caret).
pub type EditorMenuProvider = Rc<dyn Fn(&Editor, &App) -> Vec<MenuEntry>>;

/// What a command, ribbon or menu action can reach.
pub struct ActionCtx<'a> {
    pub window: &'a mut Window,
    pub cx: &'a mut App,
    /// The active editor, if any.
    pub editor: Option<Entity<Editor>>,
}

impl ActionCtx<'_> {
    /// The window's workspace (open notes, sidebar views, the vault index).
    pub fn workspace(&self) -> Option<WorkspaceHandle> {
        WorkspaceHandle::current(self.cx)
    }
}

// ---- workspace access ---------------------------------------------------------------------

/// Where to put the caret when opening a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenAt {
    /// Keep the note's current position (or the start, if it wasn't open).
    Keep,
    /// A byte offset in the note's text.
    Offset(usize),
    /// `Heading` or `^block-id` (the part after `#` in a link).
    Subpath(String),
}

/// The workspace as plugins see it (D5's `WorkspaceApi` + read-only `IndexApi`): open notes,
/// reveal sidebar views, read the vault index. File changes go through the workspace, which
/// owns conflict checks and link updates.
#[derive(Clone)]
pub struct WorkspaceHandle(pub(crate) WeakEntity<crate::workspace::Workspace>);

struct CurrentWorkspace(WeakEntity<crate::workspace::Workspace>);
impl Global for CurrentWorkspace {}

impl WorkspaceHandle {
    /// Called once by the workspace when it is created (one window per vault).
    pub(crate) fn install(ws: WeakEntity<crate::workspace::Workspace>, cx: &mut App) {
        cx.set_global(CurrentWorkspace(ws));
    }
    pub fn current(cx: &App) -> Option<Self> {
        cx.try_global::<CurrentWorkspace>().map(|c| Self(c.0.clone()))
    }
    /// The open vault (index and attachment handling). Borrow it briefly: never across a call
    /// back into the workspace.
    pub fn vault(&self, cx: &App) -> Option<crate::embeds::SharedVault> {
        self.0.upgrade().map(|ws| ws.read(cx).vault.clone())
    }
    /// Vault path of the active note.
    pub fn active_note(&self, cx: &App) -> Option<String> {
        let ws = self.0.upgrade()?;
        let ws = ws.read(cx);
        let e = ws.active_editor()?;
        let path = e.read(cx).path.clone();
        Some(ws.vault.borrow().relative(&path))
    }
    /// The active editor, if any.
    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.0.upgrade()?.read(cx).active_editor()
    }
    /// Recently opened notes, most recent first (vault paths).
    pub fn recent(&self, cx: &App) -> Vec<String> {
        self.0.upgrade().map(|ws| ws.read(cx).recent.clone()).unwrap_or_default()
    }
    /// Opens a vault file (focuses its tab if open; `new_tab` otherwise adds a tab).
    pub fn open(&self, rel: &str, at: OpenAt, new_tab: bool, window: &mut Window, cx: &mut App) {
        let rel = rel.to_string();
        let _ = self.0.update(cx, |ws, cx| ws.open_rel(&rel, at, new_tab, window, cx));
    }
    /// Creates a note (vault path, `.md` added if missing; folders created) and opens it.
    pub fn create_note(&self, rel: &str, new_tab: bool, window: &mut Window, cx: &mut App) -> Result<(), String> {
        let rel = rel.to_string();
        self.0.update(cx, |ws, cx| ws.create_and_open(&rel, new_tab, window, cx)).map_err(|e| e.to_string())?
    }
    /// Creates a note with `text` (when it doesn't exist yet) and opens it.
    pub fn create_note_with(&self, rel: &str, text: &str, new_tab: bool, window: &mut Window, cx: &mut App) -> Result<(), String> {
        let (rel, text) = (rel.to_string(), text.to_string());
        self.0.update(cx, |ws, cx| ws.create_with_text(&rel, &text, new_tab, window, cx)).map_err(|e| e.to_string())?
    }
    /// Hides both sidebars (or brings them back): a distraction-free editor.
    pub fn toggle_focus_mode(&self, cx: &mut App) {
        let _ = self.0.update(cx, |ws, cx| ws.toggle_focus_mode(cx));
    }
    /// Shows a sidebar view (full id `<plugin>:<view>`), opening its sidebar; `arg` is handed
    /// to the view's next render (e.g. a search query).
    pub fn show_view(&self, id: &str, arg: Option<String>, cx: &mut App) {
        let id = id.to_string();
        let _ = self.0.update(cx, |ws, cx| ws.show_view(&id, arg, cx));
    }
}

/// Context for rendering a sidebar view. Views render while the workspace does, so read the
/// vault and the active note from here; `workspace` is for click handlers (it can't be read
/// during the render).
pub struct ViewCtx {
    pub editor: Option<Entity<Editor>>,
    pub workspace: WorkspaceHandle,
    pub vault: crate::embeds::SharedVault,
    /// Vault path of the active note.
    pub active_note: Option<String>,
    /// Handed over by [`WorkspaceHandle::show_view`]; given to one render only.
    pub arg: Option<String>,
}

/// Reacts to files moving or disappearing (bookmarks keep pointing at renamed notes).
pub trait VaultListener {
    /// A file or folder moved from `old` to `new` (vault paths); `moves` lists every file that
    /// moved (old -> new), i.e. all files inside a moved folder.
    fn renamed(&self, _old: &str, _new: &str, _moves: &[(String, String)], _cx: &mut App) {}
    /// A file or folder was deleted (moved to the trash).
    fn deleted(&self, _path: &str, _cx: &mut App) {}
}

pub type Action = Rc<dyn Fn(&mut ActionCtx)>;

#[derive(Clone)]
pub struct Command {
    /// Namespaced automatically as `<plugin id>:<id>`.
    pub id: String,
    pub name: String,
    /// Default hotkey, e.g. `ctrl-shift-l` (user overrides arrive with the command palette, M4).
    pub hotkey: Option<String>,
    pub run: Action,
}

#[derive(Clone)]
pub struct RibbonItem {
    pub id: String,
    pub icon: IconName,
    pub tooltip: String,
    pub run: Action,
}

/// A status bar entry; `text` is recomputed on every render from the active editor.
#[derive(Clone)]
pub struct StatusItem {
    pub id: String,
    pub text: Rc<dyn Fn(Option<&Editor>) -> Option<SharedString>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// With Files (Obsidian: Search, Bookmarks).
    Left,
    /// Obsidian: Backlinks, Outgoing links, Outline, Tags.
    Right,
}

#[derive(Clone)]
pub struct SidebarView {
    /// Namespaced automatically as `<plugin id>:<id>`.
    pub id: String,
    pub title: String,
    pub icon: IconName,
    pub side: Side,
    /// Called on every render of the window: keep it cheap (cache by document version or
    /// index generation), or keep state in an entity created on first use.
    pub render: Rc<dyn Fn(&ViewCtx, &mut Window, &mut App) -> AnyElement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTarget {
    /// File tree entries (the vault path is passed to the action via `MenuCtx`).
    File,
    /// The editor's right-click menu.
    Editor,
}

#[derive(Clone)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub target: MenuTarget,
    pub run: Rc<dyn Fn(&mut ActionCtx, Option<&str>)>,
}

/// A toolbar drawn above the editor of the focused pane (e.g. the editing toolbar).
#[derive(Clone)]
pub struct ToolbarSlot {
    pub id: String,
    pub render: Rc<dyn Fn(&ViewCtx, &mut Window, &mut App) -> AnyElement>,
}

/// A custom settings section, shown under the plugin's generated settings.
#[derive(Clone)]
pub struct SettingsTab {
    pub id: String,
    pub title: String,
    pub render: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}

// ---- registrar --------------------------------------------------------------------------

/// Everything plugins registered, each entry tagged with its plugin. Unloading a plugin drops
/// all of its entries (see [`Registry::remove_plugin`]).
#[derive(Default, Clone)]
pub struct Registry {
    pub callout_renderers: Vec<(PluginId, Rc<dyn CalloutRenderer>)>,
    pub code_block_renderers: Vec<(PluginId, String, Rc<dyn CodeBlockRenderer>)>,
    pub editor_hooks: Vec<(PluginId, Rc<dyn EditorHook>)>,
    pub commands: Vec<(PluginId, Command)>,
    pub ribbon: Vec<(PluginId, RibbonItem)>,
    pub status: Vec<(PluginId, StatusItem)>,
    pub sidebar_views: Vec<(PluginId, SidebarView)>,
    pub menu_items: Vec<(PluginId, MenuItem)>,
    pub settings_tabs: Vec<(PluginId, SettingsTab)>,
    pub decorators: Vec<(PluginId, Rc<dyn TextDecorator>)>,
    pub link_labelers: Vec<(PluginId, Rc<dyn LinkLabeler>)>,
    pub editor_menus: Vec<(PluginId, EditorMenuProvider)>,
    pub vault_listeners: Vec<(PluginId, Rc<dyn VaultListener>)>,
    pub toolbars: Vec<(PluginId, ToolbarSlot)>,
}

impl Registry {
    pub fn remove_plugin(&mut self, id: &str) {
        self.callout_renderers.retain(|(p, _)| *p != id);
        self.code_block_renderers.retain(|(p, _, _)| *p != id);
        self.editor_hooks.retain(|(p, _)| *p != id);
        self.commands.retain(|(p, _)| *p != id);
        self.ribbon.retain(|(p, _)| *p != id);
        self.status.retain(|(p, _)| *p != id);
        self.sidebar_views.retain(|(p, _)| *p != id);
        self.menu_items.retain(|(p, _)| *p != id);
        self.settings_tabs.retain(|(p, _)| *p != id);
        self.decorators.retain(|(p, _)| *p != id);
        self.link_labelers.retain(|(p, _)| *p != id);
        self.editor_menus.retain(|(p, _)| *p != id);
        self.vault_listeners.retain(|(p, _)| *p != id);
        self.toolbars.retain(|(p, _)| *p != id);
    }

    /// How many registrations a plugin currently holds (tests: unload must leave zero).
    pub fn count(&self, id: &str) -> usize {
        self.callout_renderers.iter().filter(|(p, _)| *p == id).count()
            + self.code_block_renderers.iter().filter(|(p, _, _)| *p == id).count()
            + self.editor_hooks.iter().filter(|(p, _)| *p == id).count()
            + self.commands.iter().filter(|(p, _)| *p == id).count()
            + self.ribbon.iter().filter(|(p, _)| *p == id).count()
            + self.status.iter().filter(|(p, _)| *p == id).count()
            + self.sidebar_views.iter().filter(|(p, _)| *p == id).count()
            + self.menu_items.iter().filter(|(p, _)| *p == id).count()
            + self.settings_tabs.iter().filter(|(p, _)| *p == id).count()
            + self.decorators.iter().filter(|(p, _)| *p == id).count()
            + self.link_labelers.iter().filter(|(p, _)| *p == id).count()
            + self.editor_menus.iter().filter(|(p, _)| *p == id).count()
            + self.vault_listeners.iter().filter(|(p, _)| *p == id).count()
            + self.toolbars.iter().filter(|(p, _)| *p == id).count()
    }

    /// The first plugin style claiming a callout type (plugins in load order).
    pub fn callout_style(&self, kind: &str, meta: &str, dark: bool) -> Option<CalloutStyle> {
        self.callout_renderers.iter().find_map(|(_, r)| r.style(kind, meta, dark))
    }

    /// The first plugin claiming a callout type (for its numbers and body).
    pub fn callout_renderer(&self, kind: &str, meta: &str) -> Option<Rc<dyn CalloutRenderer>> {
        self.callout_renderers.iter().find(|(_, r)| r.style(kind, meta, false).is_some()).map(|(_, r)| r.clone())
    }

    pub fn code_block_renderer(&self, lang: &str) -> Option<Rc<dyn CodeBlockRenderer>> {
        self.code_block_renderers.iter().find(|(_, l, _)| l.eq_ignore_ascii_case(lang)).map(|(_, _, r)| r.clone())
    }
}

/// Handed to `Plugin::on_load`; tags everything with the plugin's id.
pub struct Registrar<'a> {
    pub(crate) plugin: PluginId,
    pub(crate) reg: &'a mut Registry,
}

impl Registrar<'_> {
    pub fn callout_renderer(&mut self, r: impl CalloutRenderer + 'static) {
        self.reg.callout_renderers.push((self.plugin, Rc::new(r)));
    }
    pub fn code_block_renderer(&mut self, lang: &str, r: impl CodeBlockRenderer + 'static) {
        self.reg.code_block_renderers.push((self.plugin, lang.to_string(), Rc::new(r)));
    }
    pub fn editor_hook(&mut self, h: Rc<dyn EditorHook>) {
        self.reg.editor_hooks.push((self.plugin, h));
    }
    pub fn command(&mut self, mut c: Command) {
        c.id = format!("{}:{}", self.plugin, c.id);
        self.reg.commands.push((self.plugin, c));
    }
    pub fn ribbon(&mut self, r: RibbonItem) {
        self.reg.ribbon.push((self.plugin, r));
    }
    pub fn status_item(&mut self, s: StatusItem) {
        self.reg.status.push((self.plugin, s));
    }
    pub fn sidebar_view(&mut self, mut v: SidebarView) {
        v.id = format!("{}:{}", self.plugin, v.id);
        self.reg.sidebar_views.push((self.plugin, v));
    }
    pub fn menu_item(&mut self, m: MenuItem) {
        self.reg.menu_items.push((self.plugin, m));
    }
    pub fn settings_tab(&mut self, t: SettingsTab) {
        self.reg.settings_tabs.push((self.plugin, t));
    }
    pub fn link_labeler(&mut self, l: Rc<dyn LinkLabeler>) {
        self.reg.link_labelers.push((self.plugin, l));
    }
    pub fn text_decorator(&mut self, d: Rc<dyn TextDecorator>) {
        self.reg.decorators.push((self.plugin, d));
    }
    pub fn editor_menu(&mut self, m: EditorMenuProvider) {
        self.reg.editor_menus.push((self.plugin, m));
    }
    pub fn toolbar(&mut self, t: ToolbarSlot) {
        self.reg.toolbars.push((self.plugin, t));
    }
    pub fn vault_listener(&mut self, l: Rc<dyn VaultListener>) {
        self.reg.vault_listeners.push((self.plugin, l));
    }
}
