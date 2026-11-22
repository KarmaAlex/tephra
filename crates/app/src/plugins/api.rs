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

pub trait CalloutRenderer {
    /// Style for `kind` (lowercased) with `|meta`, or None to leave it to others / the core.
    fn style(&self, kind: &str, meta: &str, dark: bool) -> Option<CalloutStyle>;
    /// Labels for a note's callouts by header line (numbering: "Theorem 1.2").
    fn numbers(&self, _text: &str) -> std::collections::HashMap<usize, String> {
        Default::default()
    }
    /// Draws a callout's body itself (e.g. pseudocode); `body` = its lines without quote markers.
    fn render_body(&self, _kind: &str, _meta: &str, _body: &[String], _md: &MarkdownRenderer, _window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        None
    }
    /// Something to show after the body (e.g. the end-of-proof mark).
    fn body_end(&self, _kind: &str) -> Option<String> {
        None
    }
}

/// The note an internal link points into.
pub enum LinkTarget<'a> {
    /// The note being edited (its current, possibly unsaved text).
    Text(&'a str),
    /// Another note, from the vault index.
    Meta(&'a obsidilight_core::index::NoteMeta),
}

/// Display text for internal links without an alias (references: `[[#^lemma1]]` → "lem 1.1").
pub trait LinkLabeler {
    /// `sub` is the part after `#` (e.g. `^lemma1`).
    fn label(&self, target: &LinkTarget, sub: &str) -> Option<String>;
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

#[derive(Clone)]
pub struct SidebarView {
    pub id: String,
    pub title: String,
    pub icon: IconName,
    pub render: Rc<dyn Fn(Option<&Entity<Editor>>, &mut Window, &mut App) -> AnyElement>,
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
    pub fn sidebar_view(&mut self, v: SidebarView) {
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
}
