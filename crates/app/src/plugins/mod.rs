//! Plugin host: the bundled plugins, which are enabled, their settings, and the registry of
//! everything loaded plugins registered. State lives in the vault, next to (never inside)
//! `.obsidian/`:
//!   .obsidilight/plugins.json        {"enabled": ["latex-suite", …]}
//!   .obsidilight/plugins/<id>.json   that plugin's settings

pub mod academic_notes;
pub mod api;
pub mod bookmarks;
pub mod charts;
pub mod command_palette;
pub mod copy_block_link;
pub mod daily_notes;
pub mod desmos;
pub mod editing_toolbar;
pub mod file_recovery;
pub mod horizontal_blocks;
pub mod latex_suite;
pub mod links_panes;
pub mod note_composer;
pub mod outline_tags;
pub mod pdf_export;
pub mod picker;
pub mod search;
pub mod spellcheck;
pub mod switcher;
pub mod templates;
pub mod word_count;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::*;
use serde_json::{Map, Value};

pub use api::*;

pub struct PluginHost {
    plugins: Vec<Box<dyn Plugin>>,
    enabled: BTreeSet<String>,
    loaded: HashSet<&'static str>,
    pub reg: Registry,
    settings: HashMap<String, Map<String, Value>>,
    /// `<vault>/.obsidilight`; None in tests that must not write.
    dir: Option<PathBuf>,
    /// Bumped whenever registrations change, so views re-render/re-project.
    pub generation: u64,
    /// Your hotkeys by command id (`.obsidilight/hotkeys.json`); a command without an entry
    /// keeps its default, an empty list means no hotkey.
    hotkeys: HashMap<String, Vec<String>>,
}

impl Global for PluginHost {}

/// Runs a plugin action with the active editor.
pub fn run_action(run: &api::Action, editor: Option<Entity<crate::editor::Editor>>, window: &mut Window, cx: &mut App) {
    let mut ctx = ActionCtx { window, cx, editor };
    run(&mut ctx);
}

/// Every plugin shipped with the app, in load order (earlier plugins win style conflicts;
/// sidebar views appear in this order).
pub fn bundled(vault_root: &Path) -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(latex_suite::LatexSuite::new(vault_root)),
        Box::new(spellcheck::SpellCheck::new(vault_root)),
        Box::new(search::Search::new()),
        Box::new(switcher::Switcher::new()),
        Box::new(command_palette::CommandPalette::new()),
        Box::new(daily_notes::DailyNotes::new(vault_root)),
        Box::new(templates::Templates::new(vault_root)),
        Box::new(note_composer::NoteComposer::new(vault_root)),
        Box::new(file_recovery::FileRecovery::new(vault_root)),
        Box::new(bookmarks::Bookmarks::new(vault_root)),
        Box::new(links_panes::Backlinks::new()),
        Box::new(links_panes::OutgoingLinks::new()),
        Box::new(outline_tags::Outline::new()),
        Box::new(outline_tags::TagPane::new()),
        Box::new(academic_notes::AcademicNotes::new(vault_root)),
        Box::new(copy_block_link::CopyBlockLink::new()),
        Box::new(editing_toolbar::EditingToolbar::new(vault_root)),
        Box::new(charts::Charts::new()),
        Box::new(pdf_export::PdfExport::new(vault_root)),
        Box::new(word_count::WordCount::new()),
        Box::new(desmos::Desmos::new()),
        Box::new(horizontal_blocks::HorizontalBlocks::new(vault_root)),
    ]
}

/// The bundled plugins before `plugins.json` recorded which plugins it knew (M2–M3).
const LEGACY_PLUGINS: &[&str] = &["academic-notes", "latex-suite", "spellcheck", "word-count", "desmos", "horizontal-blocks"];

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.obsidilight-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

impl PluginHost {
    pub fn new(dir: Option<PathBuf>, plugins: Vec<Box<dyn Plugin>>) -> Self {
        let read_json = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
        // Saved choices win; plugins that didn't exist when the file was written (newer app
        // version) get their default. Files from before "known" was recorded (M2/M3) knew these.
        let strs = |v: Option<&Value>| -> Option<BTreeSet<String>> { v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()) };
        let saved = dir.as_ref().and_then(|d| read_json(d.join("plugins.json")));
        let saved_enabled = strs(saved.as_ref().and_then(|v| v.get("enabled"))).unwrap_or_default();
        let known = match &saved {
            Some(v) => strs(v.get("known")).unwrap_or_else(|| LEGACY_PLUGINS.iter().map(|s| s.to_string()).collect()),
            None => BTreeSet::new(),
        };
        let enabled: BTreeSet<String> = plugins
            .iter()
            .map(|p| p.manifest())
            .filter(|m| if known.contains(m.id) { saved_enabled.contains(m.id) } else { m.enabled_by_default })
            .map(|m| m.id.to_string())
            .collect();
        let mut settings = HashMap::new();
        for p in &plugins {
            let m = p.manifest();
            let mut values: Map<String, Value> = m.settings.iter().map(|s| (s.key.to_string(), s.default.clone())).collect();
            if let Some(Value::Object(saved)) = dir.as_ref().and_then(|d| read_json(d.join("plugins").join(format!("{}.json", m.id)))) {
                for (k, v) in saved {
                    values.insert(k, v); // unknown keys are preserved
                }
            }
            settings.insert(m.id.to_string(), values);
        }
        let hotkeys: HashMap<String, Vec<String>> = dir
            .as_ref()
            .and_then(|d| read_json(d.join("hotkeys.json")))
            .and_then(|v| v.as_object().cloned())
            .map(|o| o.into_iter().map(|(k, v)| (k, v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default())).collect())
            .unwrap_or_default();
        Self { plugins, enabled, loaded: HashSet::new(), reg: Registry::default(), settings, dir, generation: 0, hotkeys }
    }

    /// Installs the host for this window's vault and loads the enabled plugins.
    pub fn init(cx: &mut App, vault_root: &Path) {
        let mut host = Self::new(Some(vault_root.join(".obsidilight")), bundled(vault_root));
        let ids: Vec<&'static str> = host.plugins.iter().map(|p| p.manifest().id).filter(|id| host.enabled.contains(*id)).collect();
        let timing = std::env::var_os("OBSIDILIGHT_TIMING").is_some();
        for id in ids {
            let t = std::time::Instant::now();
            host.load_plugin(id);
            if timing {
                eprintln!("TIMING plugin {id} {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        cx.set_global(host);
    }

    /// Registrations owned by the app itself (`id` = "app"): never unloaded.
    pub fn register_core(&mut self, id: PluginId, f: impl FnOnce(&mut Registrar)) {
        if self.reg.count(id) == 0 {
            f(&mut Registrar { plugin: id, reg: &mut self.reg });
            self.generation += 1;
        }
    }

    /// The hotkeys a command answers to: yours, else its default.
    pub fn hotkeys_of(&self, c: &Command) -> Vec<String> {
        match self.hotkeys.get(&c.id) {
            Some(v) => v.clone(),
            None => c.hotkey.iter().cloned().collect(),
        }
    }

    /// Whether you changed this command's hotkeys.
    pub fn hotkey_overridden(&self, id: &str) -> bool {
        self.hotkeys.contains_key(id)
    }

    /// The command whose hotkey this is.
    pub fn command_for(&self, k: &Keystroke) -> Option<Command> {
        self.reg.commands.iter().map(|(_, c)| c).find(|c| self.hotkeys_of(c).iter().any(|h| crate::hotkeys::matches(h, k))).cloned()
    }

    fn apply_hotkeys(&mut self, id: &str, keys: Option<Vec<String>>) {
        match keys {
            Some(v) => self.hotkeys.insert(id.to_string(), v),
            None => self.hotkeys.remove(id),
        };
        if let Some(d) = &self.dir {
            let v: Map<String, Value> = self.hotkeys.iter().map(|(k, v)| (k.clone(), Value::from(v.clone()))).collect();
            let _ = write_atomic(&d.join("hotkeys.json"), &serde_json::to_string_pretty(&Value::Object(v)).unwrap_or_default());
        }
        self.generation += 1;
    }

    /// Sets a command's hotkeys (None: back to the default); saved and applied at once.
    pub fn set_hotkeys(cx: &mut App, id: &str, keys: Option<Vec<String>>) {
        cx.global_mut::<PluginHost>().apply_hotkeys(id, keys);
        cx.refresh_windows();
    }

    /// Display name of a plugin (None for the app's own registrations).
    pub fn plugin_name(&self, id: &str) -> Option<&'static str> {
        self.plugins.iter().map(|p| p.manifest()).find(|m| m.id == id).map(|m| m.name)
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.plugins.iter().map(|p| p.manifest().clone()).collect()
    }
    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.contains(id)
    }
    #[cfg(test)]
    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded.contains(id)
    }
    /// How the editor should show a fenced code block of `lang`.
    pub fn code_mode(&self, lang: &str) -> crate::editor::projection::CodeMode {
        use crate::editor::projection::CodeMode;
        if lang.is_empty() {
            return CodeMode::Plain;
        }
        if self.reg.code_block_renderer(lang).is_some() {
            return CodeMode::Rendered;
        }
        match self.plugins.iter().map(|p| p.manifest()).find(|m| m.code_languages.iter().any(|l| l.eq_ignore_ascii_case(lang))) {
            Some(m) if self.loaded.contains(m.id) => CodeMode::Note(format!("{}: rendering is not available yet", m.name)),
            Some(m) => CodeMode::Note(format!("{} plugin disabled", m.name)),
            None => CodeMode::Plain,
        }
    }

    pub fn setting(&self, id: &str, key: &str) -> Value {
        self.settings.get(id).and_then(|m| m.get(key)).cloned().unwrap_or(Value::Null)
    }

    fn persist_enabled(&self) {
        if let Some(d) = &self.dir {
            let known: Vec<&str> = self.plugins.iter().map(|p| p.manifest().id).collect();
            let v = serde_json::json!({ "enabled": self.enabled.iter().collect::<Vec<_>>(), "known": known });
            let _ = write_atomic(&d.join("plugins.json"), &serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }

    fn persist_settings(&self, id: &str) {
        if let (Some(d), Some(m)) = (&self.dir, self.settings.get(id)) {
            let _ = write_atomic(&d.join("plugins").join(format!("{id}.json")), &serde_json::to_string_pretty(m).unwrap_or_default());
        }
    }

    /// Loads one plugin (no-op if loaded or unknown); true if something changed.
    fn load_plugin(&mut self, id: &str) -> bool {
        if self.loaded.contains(id) {
            return false;
        }
        let Some(ix) = self.plugins.iter().position(|p| p.manifest().id == id) else { return false };
        let pid = self.plugins[ix].manifest().id;
        let settings = self.settings.get(id).cloned().unwrap_or_default();
        let mut r = Registrar { plugin: pid, reg: &mut self.reg };
        self.plugins[ix].on_load(&settings, &mut r);
        self.loaded.insert(pid);
        self.generation += 1;
        true
    }

    /// Unloads one plugin and drops everything it registered.
    fn unload_plugin(&mut self, id: &str) -> bool {
        if !self.loaded.remove(id) {
            return false;
        }
        if let Some(p) = self.plugins.iter_mut().find(|p| p.manifest().id == id) {
            p.on_unload();
        }
        self.reg.remove_plugin(id);
        self.generation += 1;
        true
    }

    fn apply_enabled(&mut self, id: &str, on: bool) {
        if on {
            self.enabled.insert(id.to_string());
        } else {
            self.enabled.remove(id);
        }
        self.persist_enabled();
        if on {
            self.load_plugin(id);
        } else {
            self.unload_plugin(id);
        }
    }

    fn apply_setting(&mut self, id: &str, key: &str, value: Value) {
        self.settings.entry(id.to_string()).or_default().insert(key.to_string(), value);
        self.persist_settings(id);
        if self.unload_plugin(id) {
            self.load_plugin(id);
        }
    }

    /// Enables/disables a plugin; applies immediately (no restart) and is saved.
    pub fn set_enabled(cx: &mut App, id: &str, on: bool) {
        cx.global_mut::<PluginHost>().apply_enabled(id, on);
        cx.refresh_windows();
    }

    /// Changes several settings at once (one save, one reload).
    pub fn set_settings(cx: &mut App, id: &str, values: Vec<(&str, Value)>) {
        let host = cx.global_mut::<PluginHost>();
        for (k, v) in values {
            host.settings.entry(id.to_string()).or_default().insert(k.to_string(), v);
        }
        host.persist_settings(id);
        if host.unload_plugin(id) {
            host.load_plugin(id);
        }
        cx.refresh_windows();
    }

    /// Changes one setting, saves it, and reloads the plugin so it takes effect.
    pub fn set_setting(cx: &mut App, id: &str, key: &str, value: Value) {
        cx.global_mut::<PluginHost>().apply_setting(id, key, value);
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    // no `gpui_kit::*` glob here: its `test` macro would shadow the built-in one
    use super::api::*;
    use super::PluginHost;
    use gpui_kit::{div, AnyElement, App, IntoElement as _, Window};
    use serde_json::{Map, Value};
    use std::rc::Rc;

    /// Registers one of everything.
    struct KitchenSink(PluginManifest);
    struct Nop;
    impl CalloutRenderer for Nop {
        fn style(&self, _: &str, _: &str, _: bool) -> Option<CalloutStyle> {
            None
        }
    }
    impl CodeBlockRenderer for Nop {
        fn render(&self, _: &str, _: &MarkdownRenderer, _: &mut Window, _: &mut App) -> AnyElement {
            div().into_any_element()
        }
    }
    impl EditorHook for Nop {}
    impl LinkLabeler for Nop {
        fn label(&self, _: &NoteCtx, _: &str) -> Option<String> {
            None
        }
    }
    impl VaultListener for Nop {}
    impl TextDecorator for Nop {
        fn decorate(&self, _: &str) -> Vec<std::ops::Range<usize>> {
            Vec::new()
        }
    }
    impl Plugin for KitchenSink {
        fn manifest(&self) -> &PluginManifest {
            &self.0
        }
        fn on_load(&mut self, _: &Map<String, Value>, r: &mut Registrar) {
            let run: Action = Rc::new(|_| {});
            r.callout_renderer(Nop);
            r.code_block_renderer("x", Nop);
            r.editor_hook(Rc::new(Nop));
            r.command(Command { id: "c".into(), name: "C".into(), hotkey: Some("ctrl-alt-k".into()), run: run.clone() });
            r.ribbon(RibbonItem { id: "r".into(), icon: gpui_kit::assets::IconName::Star, tooltip: "R".into(), run: run.clone() });
            r.status_item(StatusItem { id: "s".into(), text: Rc::new(|_| None) });
            r.sidebar_view(SidebarView { id: "v".into(), title: "V".into(), icon: gpui_kit::assets::IconName::Star, side: Side::Right, render: Rc::new(|_, _, _| div().into_any_element()) });
            r.menu_item(MenuItem { id: "m".into(), label: "M".into(), target: MenuTarget::File, run: Rc::new(|_, _| {}) });
            r.settings_tab(SettingsTab { id: "t".into(), title: "T".into(), render: Rc::new(|_, _| div().into_any_element()) });
            r.text_decorator(Rc::new(Nop));
            r.link_labeler(Rc::new(Nop));
            r.editor_menu(Rc::new(|_, _| Vec::new()));
            r.vault_listener(Rc::new(Nop));
            r.toolbar(ToolbarSlot { id: "t".into(), render: Rc::new(|_, _, _| div().into_any_element()) });
        }
    }

    fn manifest(id: &'static str, on: bool) -> PluginManifest {
        PluginManifest { id, name: id, version: "0", description: "", enabled_by_default: on, settings: vec![SettingSpec { key: "k", label: "K", description: "", kind: SettingKind::Toggle, default: Value::Bool(true) }], code_languages: &[] }
    }

    #[test]
    fn toggling_registers_and_unregisters_everything() {
        let mut h = PluginHost::new(None, vec![Box::new(KitchenSink(manifest("a", true))), Box::new(KitchenSink(manifest("b", false)))]);
        assert!(h.load_plugin("a"));
        assert!(!h.load_plugin("a"), "loading twice is a no-op");
        assert_eq!(h.reg.count("a"), 14);
        assert_eq!(h.reg.commands[0].1.id, "a:c");
        h.apply_enabled("b", true);
        assert_eq!(h.reg.count("b"), 14);
        let g = h.generation;
        h.apply_enabled("a", false);
        assert_eq!(h.reg.count("a"), 0, "unload must remove every registration");
        assert_eq!(h.reg.count("b"), 14, "other plugins untouched");
        assert!(!h.is_loaded("a") && h.is_loaded("b") && !h.is_enabled("a"));
        assert!(h.generation > g);
        h.apply_setting("b", "k", Value::Bool(false)); // reloads b
        assert_eq!((h.reg.count("b"), h.setting("b", "k")), (14, Value::Bool(false)));
    }

    #[test]
    fn settings_persist_with_defaults_and_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("ol-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        std::fs::write(dir.join("plugins/a.json"), r#"{"k": false, "future": 3}"#).unwrap();
        let host = PluginHost::new(Some(dir.clone()), vec![Box::new(KitchenSink(manifest("a", true)))]);
        assert_eq!(host.setting("a", "k"), Value::Bool(false));
        assert_eq!(host.setting("a", "future"), serde_json::json!(3));
        assert!(host.is_enabled("a")); // no plugins.json yet: defaults
        // a saved choice is kept; a plugin the file didn't know gets its default
        std::fs::write(dir.join("plugins.json"), r#"{"enabled": [], "known": ["a"]}"#).unwrap();
        let host2 = PluginHost::new(Some(dir.clone()), vec![Box::new(KitchenSink(manifest("a", true))), Box::new(KitchenSink(manifest("new", true)))]);
        assert!(!host2.is_enabled("a") && host2.is_enabled("new"));
        std::fs::write(dir.join("plugins.json"), r#"{"enabled": ["spellcheck"]}"#).unwrap();
        let host3 = PluginHost::new(Some(dir.clone()), vec![Box::new(KitchenSink(manifest("word-count", true))), Box::new(KitchenSink(manifest("new", true)))]);
        assert!(!host3.is_enabled("word-count") && host3.is_enabled("new"), "M3-era file: legacy ids known");
        // hotkey overrides: saved, and an empty list removes the default
        let mut h = PluginHost::new(Some(dir.clone()), vec![Box::new(KitchenSink(manifest("a", true)))]);
        h.load_plugin("a");
        let c = h.reg.commands[0].1.clone();
        assert_eq!(h.hotkeys_of(&c), vec!["ctrl-alt-k"]);
        h.apply_hotkeys("a:c", Some(vec!["ctrl-shift-j".into()]));
        let h2 = PluginHost::new(Some(dir.clone()), vec![]);
        assert_eq!(h2.hotkeys_of(&c), vec!["ctrl-shift-j"]);
        h.apply_hotkeys("a:c", Some(vec![]));
        assert!(h.hotkeys_of(&c).is_empty());
        h.apply_hotkeys("a:c", None);
        assert_eq!(h.hotkeys_of(&c), vec!["ctrl-alt-k"]);
        host.persist_settings("a");
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("plugins/a.json")).unwrap()).unwrap();
        assert_eq!(saved["future"], serde_json::json!(3));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
