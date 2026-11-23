//! Themes and CSS snippets (HANDOFF §5.1 "Theming"), read the way you chose in M6: GPUI has no
//! CSS engine, so what applies is Obsidian's CSS *variables*:
//! - declared in global rules (`:root`, `body`, `.theme-light`, `.theme-dark`) of the chosen
//!   theme (`.obsidian/themes/<name>/theme.css`) and of the enabled snippets;
//! - declared in rules scoped by one class (`.NoBorder { … }`), from every snippet, for notes
//!   whose `cssclasses` name it (note-specific snippets apply without being enabled);
//! - `--callout-color` / `--callout-icon` in `.callout[data-callout="type"]` rules.
//!
//! Everything else (rules that style elements directly, `@media`, `@font-face`, …) is ignored
//! and counted, per file, for Settings → Appearance. Under it all sit built-in light and dark
//! variables following Obsidian's default theme. Choices live in `.obsidilight/app.json`
//! (`baseTheme`, `cssTheme`, `enabledCssSnippets`); the defaults come from
//! `.obsidian/appearance.json` (read, never written).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gpui_kit::base::text::TextViewStyle;
use gpui_kit::*;
use serde_json::Value;

use crate::css::{eval_calc, parse_color, parse_length, parse_sheet, substitute_vars, Sheet};

/// Obsidian's default theme, as variables (values from the default light and dark themes).
const BASE: &str = r#"
.theme-light {
  --mono-rgb-0: 255, 255, 255; --mono-rgb-100: 0, 0, 0;
  --color-base-00: #ffffff; --color-base-05: #fcfcfc; --color-base-10: #fafafa; --color-base-20: #f6f6f6;
  --color-base-25: #e3e3e3; --color-base-30: #e0e0e0; --color-base-35: #d4d4d4; --color-base-40: #bdbdbd;
  --color-base-50: #ababab; --color-base-60: #707070; --color-base-70: #5c5c5c; --color-base-100: #222222;
  --color-red-rgb: 233, 49, 71; --color-orange-rgb: 236, 117, 0; --color-yellow-rgb: 224, 172, 0;
  --color-green-rgb: 8, 185, 78; --color-cyan-rgb: 0, 191, 188; --color-blue-rgb: 8, 109, 221;
  --color-purple-rgb: 120, 82, 238; --color-pink-rgb: 213, 57, 132;
  --accent-h: 258; --accent-s: 88%; --accent-l: 66%;
  --text-highlight-bg: rgba(255, 208, 0, 0.4);
  --background-modifier-hover: rgba(var(--mono-rgb-100), 0.075);
  --text-selection: hsla(var(--accent-h), var(--accent-s), var(--accent-l), 0.25);
}
.theme-dark {
  --mono-rgb-0: 0, 0, 0; --mono-rgb-100: 255, 255, 255;
  --color-base-00: #1e1e1e; --color-base-05: #212121; --color-base-10: #242424; --color-base-20: #262626;
  --color-base-25: #2a2a2a; --color-base-30: #363636; --color-base-35: #3f3f3f; --color-base-40: #555555;
  --color-base-50: #666666; --color-base-60: #999999; --color-base-70: #b3b3b3; --color-base-100: #dadada;
  --color-red-rgb: 251, 70, 76; --color-orange-rgb: 233, 151, 63; --color-yellow-rgb: 224, 222, 113;
  --color-green-rgb: 68, 207, 110; --color-cyan-rgb: 83, 223, 221; --color-blue-rgb: 2, 122, 255;
  --color-purple-rgb: 168, 130, 255; --color-pink-rgb: 250, 153, 205;
  --accent-h: 258; --accent-s: 88%; --accent-l: 66%;
  --text-highlight-bg: rgba(255, 208, 0, 0.4);
  --background-modifier-hover: rgba(var(--mono-rgb-100), 0.075);
  --text-selection: hsla(var(--accent-h), 50%, 40%, 0.4);
}
body {
  --background-primary: var(--color-base-00);
  --background-primary-alt: var(--color-base-10);
  --background-secondary: var(--color-base-20);
  --background-secondary-alt: var(--color-base-05);
  --background-modifier-border: var(--color-base-30);
  --background-modifier-border-hover: var(--color-base-35);
  --text-normal: var(--color-base-100);
  --text-muted: var(--color-base-70);
  --text-faint: var(--color-base-50);
  --text-on-accent: white;
  --color-accent: hsl(var(--accent-h), var(--accent-s), var(--accent-l));
  --interactive-accent: var(--color-accent);
  --interactive-accent-hover: hsl(var(--accent-h), var(--accent-s), calc(var(--accent-l) - 4%));
  --text-accent: var(--color-accent);
  --link-color: var(--text-accent);
  --link-unresolved-color: var(--text-accent);
  --caret-color: var(--text-normal);
  --code-background: var(--background-primary-alt);
  --code-normal: var(--text-normal);
  --blockquote-border-color: var(--interactive-accent);
  --blockquote-border-thickness: 2px;
  --hr-color: var(--background-modifier-border);
  --hr-thickness: 2px;
  --table-border-width: 1px;
  --table-border-color: var(--background-modifier-border);
  --table-header-background: transparent;
  --table-white-space: normal;
  --font-text-size: 16px;
  --h1-size: 1.618em; --h2-size: 1.462em; --h3-size: 1.318em; --h4-size: 1.188em; --h5-size: 1.076em; --h6-size: 1em;
  --h1-color: inherit; --h2-color: inherit; --h3-color: inherit; --h4-color: inherit; --h5-color: inherit; --h6-color: inherit;
  --h1-weight: 700; --h2-weight: 600; --h3-weight: 600; --h4-weight: 600; --h5-weight: 600; --h6-weight: 600;
  --tab-background-active: var(--background-primary);
  --titlebar-background: var(--background-secondary);
  --ribbon-background: var(--background-secondary);
}
"#;

/// Obsidian's structural classes and elements: they don't make a rule note-specific.
const STRUCTURAL: &[&str] = &[
    "theme-light", "theme-dark", "markdown-preview-view", "markdown-source-view", "markdown-rendered", "markdown-reading-view", "cm-s-obsidian", "mod-cm6", "view-content", "workspace-leaf-content",
    "is-live-preview", "markdown-preview-section", "app-container", "cm-content", "cm-editor", "cm-scroller", "workspace", "mod-root", "is-readable-line-width",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Mode {
    Both,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
enum Scope {
    Global(Mode),
    /// A note class (`cssclasses`).
    Class(String, Mode),
    /// `.callout[data-callout="type"]`.
    Callout(String, Mode),
    Other,
}

/// Where a rule applies, from its selector.
fn classify(selector: &str) -> Scope {
    let mut classes: Vec<String> = Vec::new();
    let mut callout: Option<String> = None;
    let mut other = false;
    for compound in selector.split(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~')).filter(|c| !c.is_empty()) {
        // split a compound into its simple selectors
        let mut rest = compound;
        let mut first = true;
        while !rest.is_empty() {
            let (tok, next) = match rest[1..].find(['.', ':', '[', '#']) {
                Some(i) if !first || !rest.starts_with(|c: char| c.is_alphanumeric() || c == '*') || true => {
                    // keep `[...]` and `:not(...)` whole
                    let cut = if rest.starts_with('[') { rest.find(']').map_or(rest.len(), |e| e + 1) } else if rest.starts_with(':') && rest.contains('(') { rest.find(')').map_or(rest.len(), |e| e + 1) } else { i + 1 };
                    rest.split_at(cut.min(rest.len()))
                }
                _ => (rest, ""),
            };
            first = false;
            rest = next;
            if let Some(c) = tok.strip_prefix('.') {
                classes.push(c.to_string());
            } else if let Some(a) = tok.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
                match a.split_once('=') {
                    Some((k, v)) if k.trim() == "data-callout" => callout = Some(v.trim().trim_matches(['"', '\'']).to_ascii_lowercase()),
                    _ => other = true,
                }
            } else if tok == ":root" || tok.eq_ignore_ascii_case("body") || tok.eq_ignore_ascii_case("html") || tok == "*" {
            } else if tok.starts_with(":not(") {
                // `body:not(.theme-dark)` and the like: treat as the other mode below
                if tok.contains("theme-dark") {
                    classes.push("theme-light".into());
                } else if tok.contains("theme-light") {
                    classes.push("theme-dark".into());
                } else {
                    other = true;
                }
            } else {
                other = true; // elements (table, h1…), ids, pseudo-classes
            }
        }
    }
    let mode = if classes.iter().any(|c| c == "theme-dark") {
        Mode::Dark
    } else if classes.iter().any(|c| c == "theme-light") {
        Mode::Light
    } else {
        Mode::Both
    };
    let user: Vec<&String> = classes.iter().filter(|c| !STRUCTURAL.contains(&c.as_str()) && c.as_str() != "callout").collect();
    if let Some(t) = callout.filter(|_| classes.iter().any(|c| c == "callout")) {
        return if user.is_empty() { Scope::Callout(t, mode) } else { Scope::Other };
    }
    match user.as_slice() {
        [] if !other => Scope::Global(mode),
        // `.NoBorder table { --x }`: variables inherit, so the class is what matters
        [c] => Scope::Class((*c).clone(), mode),
        _ => Scope::Other,
    }
}

/// What a theme or snippet file contributed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub name: String,
    pub is_theme: bool,
    pub enabled: bool,
    /// Global variables (applied only when enabled), note-class variables, callout settings.
    pub global_vars: usize,
    pub class_vars: usize,
    pub classes: Vec<String>,
    pub callouts: usize,
    /// Rules and at-rules that have no effect here, with a few examples.
    pub ignored: usize,
    pub examples: Vec<String>,
}

type Vars = HashMap<String, String>;

#[derive(Default)]
struct Layered {
    both: Vars,
    light: Vars,
    dark: Vars,
}

impl Layered {
    fn set(&mut self, mode: Mode, k: &str, v: &str) {
        match mode {
            Mode::Both => self.both.insert(k.to_string(), v.to_string()),
            Mode::Light => self.light.insert(k.to_string(), v.to_string()),
            Mode::Dark => self.dark.insert(k.to_string(), v.to_string()),
        };
    }
    fn get(&self, k: &str, dark: bool) -> Option<&String> {
        (if dark { &self.dark } else { &self.light }).get(k).or_else(|| self.both.get(k))
    }
}

/// The loaded theme, snippets and their reports. A GPUI global.
#[derive(Default)]
pub struct ThemeCss {
    global: Vec<Layered>,
    classes: HashMap<String, Layered>,
    callouts: HashMap<String, Layered>,
    pub reports: Vec<Report>,
    pub themes: Vec<String>,
    /// Bumped on every reload (caches key on it).
    pub generation: u64,
    pub root: PathBuf,
}

impl Global for ThemeCss {}

fn read_sheet(p: &Path) -> Option<Sheet> {
    std::fs::read_to_string(p).ok().map(|t| parse_sheet(&t))
}

/// Installed themes: `.obsidian/themes/<name>/theme.css` (and the older `<name>.css`).
fn installed_themes(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(root.join(".obsidian/themes")).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() && p.join("theme.css").exists() {
            out.insert(name, p.join("theme.css"));
        } else if let Some(n) = name.strip_suffix(".css") {
            out.insert(n.to_string(), p);
        }
    }
    out
}

/// Snippets in `.obsidian/snippets` by name (without `.css`).
pub fn installed_snippets(root: &Path) -> BTreeMap<String, PathBuf> {
    std::fs::read_dir(root.join(".obsidian/snippets"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.strip_suffix(".css").map(|s| (s.to_string(), e.path()))
        })
        .collect()
}

/// Your Obsidian appearance settings (defaults for ours).
fn obsidian_appearance(root: &Path) -> serde_json::Map<String, Value> {
    crate::plugins::templates::obsidian_core_settings(root, "appearance.json")
}

impl ThemeCss {
    /// Loads the theme and snippets as `cx`'s settings say.
    pub fn load(root: &Path, css_theme: &str, enabled: &[String], generation: u64) -> Self {
        let mut me = ThemeCss { root: root.to_path_buf(), generation, ..Default::default() };
        let mut base = Layered::default();
        me.add_sheet(&parse_sheet(BASE), &mut base, &mut Report::default(), true);
        me.global.push(base);
        let themes = installed_themes(root);
        me.themes = themes.keys().cloned().collect();
        let mut files: Vec<(String, PathBuf, bool, bool)> = Vec::new();
        if let Some(p) = themes.get(css_theme) {
            files.push((css_theme.to_string(), p.clone(), true, true));
        }
        for (name, p) in installed_snippets(root) {
            let on = enabled.contains(&name);
            files.push((name, p, false, on));
        }
        for (name, p, is_theme, on) in files {
            let Some(sheet) = read_sheet(&p) else { continue };
            let mut report = Report { name, is_theme, enabled: on, ..Default::default() };
            let mut layer = Layered::default();
            me.add_sheet(&sheet, &mut layer, &mut report, on);
            if on {
                me.global.push(layer);
            }
            me.reports.push(report);
        }
        me
    }

    fn add_sheet(&mut self, sheet: &Sheet, global: &mut Layered, report: &mut Report, enabled: bool) {
        let ignore = |report: &mut Report, what: String| {
            report.ignored += 1;
            if report.examples.len() < 4 {
                report.examples.push(what);
            }
        };
        for a in &sheet.at_rules {
            ignore(report, a.clone());
        }
        for rule in &sheet.rules {
            let vars: Vec<&(String, String)> = rule.decls.iter().filter(|(k, _)| k.starts_with("--")).collect();
            let plain: Vec<&(String, String)> = rule.decls.iter().filter(|(k, _)| !k.starts_with("--")).collect();
            let mut used = false;
            for sel in &rule.selectors {
                match classify(sel) {
                    Scope::Global(mode) if !vars.is_empty() => {
                        used = true;
                        if enabled {
                            for (k, v) in &vars {
                                global.set(mode, k, v);
                            }
                        }
                        report.global_vars += vars.len();
                    }
                    Scope::Class(c, mode) if !vars.is_empty() => {
                        used = true;
                        let layer = self.classes.entry(c.clone()).or_default();
                        for (k, v) in &vars {
                            layer.set(mode, k, v);
                        }
                        report.class_vars += vars.len();
                        if !report.classes.contains(&c) {
                            report.classes.push(c);
                        }
                    }
                    Scope::Callout(t, mode) => {
                        let cv: Vec<&&(String, String)> = vars.iter().filter(|(k, _)| k == "--callout-color" || k == "--callout-icon").collect();
                        if !cv.is_empty() && enabled {
                            used = true;
                            let layer = self.callouts.entry(t).or_default();
                            for (k, v) in cv {
                                layer.set(mode, k, v);
                            }
                            report.callouts += 1;
                        }
                    }
                    _ => {}
                }
            }
            if !plain.is_empty() || !used {
                let sel = rule.selectors.join(", ");
                let props: Vec<&str> = if plain.is_empty() { rule.decls.iter().map(|(k, _)| k.as_str()).take(3).collect() } else { plain.iter().map(|(k, _)| k.as_str()).take(3).collect() };
                ignore(report, format!("{sel} {{ {} }}", props.join("; ")));
            }
        }
    }

    /// A variable's raw value: note classes first, then enabled sheets (later wins), then base.
    fn raw(&self, name: &str, dark: bool, classes: &[String]) -> Option<String> {
        for c in classes.iter().rev() {
            if let Some(v) = self.classes.get(c).and_then(|l| l.get(name, dark)) {
                return Some(v.clone());
            }
        }
        self.global.iter().rev().find_map(|l| l.get(name, dark).cloned())
    }

    /// A variable with `var()` and `calc()` resolved; None if unset or "inherit".
    pub fn var(&self, name: &str, dark: bool, classes: &[String]) -> Option<String> {
        let raw = self.raw(name, dark, classes)?;
        let v = substitute_vars(&raw, &|n| self.raw(n, dark, classes), 0)?;
        let v = eval_calc(&v).trim().to_string();
        (!v.is_empty() && v != "inherit" && v != "initial" && v != "unset").then_some(v)
    }

    pub fn color(&self, name: &str, dark: bool, classes: &[String]) -> Option<Hsla> {
        parse_color(&self.var(name, dark, classes)?)
    }

    /// A length in px (`em` relative to `em`).
    pub fn length(&self, name: &str, dark: bool, classes: &[String], em: f32) -> Option<f32> {
        parse_length(&self.var(name, dark, classes)?, em)
    }

    /// A callout type's colour from a snippet (`--callout-color: r, g, b`).
    pub fn callout_color(&self, kind: &str, dark: bool) -> Option<Hsla> {
        let l = self.callouts.get(kind)?;
        let raw = l.get("--callout-color", dark)?;
        let v = substitute_vars(raw, &|n| self.raw(n, dark, &[]), 0)?;
        parse_color(&eval_calc(&v))
    }
}

// ---- settings and application ---------------------------------------------------------

/// `system`, `light` or `dark` (ours, else Obsidian's `theme`: obsidian = dark, moonstone = light).
pub fn base_theme(cx: &App) -> String {
    if let Some(v) = crate::app_settings::AppSettings::value(cx, "baseTheme").and_then(|v| v.as_str().map(String::from)) {
        return v;
    }
    let root = cx.try_global::<ThemeCss>().map(|t| t.root.clone()).unwrap_or_default();
    match obsidian_appearance(&root).get("theme").and_then(Value::as_str) {
        Some("obsidian") => "dark".into(),
        Some("moonstone") => "light".into(),
        _ => "system".into(),
    }
}

pub fn css_theme(cx: &App, root: &Path) -> String {
    crate::app_settings::AppSettings::value(cx, "cssTheme").and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| obsidian_appearance(root).get("cssTheme").and_then(Value::as_str).unwrap_or("").to_string())
}

pub fn enabled_snippets(cx: &App, root: &Path) -> Vec<String> {
    let strs = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>());
    crate::app_settings::AppSettings::value(cx, "enabledCssSnippets").and_then(|v| strs(&v)).unwrap_or_else(|| obsidian_appearance(root).get("enabledCssSnippets").and_then(strs).unwrap_or_default())
}

/// (Re)loads the theme and snippets for the vault and applies them.
pub fn init(cx: &mut App, root: &Path) {
    let generation = cx.try_global::<ThemeCss>().map_or(1, |t| t.generation + 1);
    let t = ThemeCss::load(root, &css_theme(cx, root), &enabled_snippets(cx, root), generation);
    cx.set_global(t);
}

pub fn reload(cx: &mut App) {
    let root = cx.try_global::<ThemeCss>().map(|t| t.root.clone()).unwrap_or_default();
    init(cx, &root);
    apply(None, cx);
}

/// Whether the dark variables apply (setting, or the system's appearance).
pub fn is_dark(window: Option<&Window>, cx: &App) -> bool {
    match base_theme(cx).as_str() {
        "dark" => true,
        "light" => false,
        _ => matches!(window.map(|w| w.appearance()).unwrap_or_else(|| cx.window_appearance()), WindowAppearance::Dark | WindowAppearance::VibrantDark),
    }
}

/// Sets the app's colours (gpui-component theme) from the variables.
pub fn apply(window: Option<&Window>, cx: &mut App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};
    let dark = is_dark(window, cx);
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    let Some(t) = cx.try_global::<ThemeCss>() else { return };
    let c = |n: &str| t.color(n, dark, &[]);
    let (bg, fg, muted, faint, bg2, border, accent, hover, sel, link) = (
        c("--background-primary"),
        c("--text-normal"),
        c("--text-muted"),
        c("--text-faint"),
        c("--background-secondary"),
        c("--background-modifier-border"),
        c("--interactive-accent"),
        c("--background-modifier-hover"),
        c("--text-selection"),
        c("--link-color"),
    );
    let accent_hover = c("--interactive-accent-hover");
    let on_accent = c("--text-on-accent");
    let tab_active = c("--tab-background-active");
    let scale = ui_scale(cx);
    Theme::update(cx, |th| {
        // the rem size: every rem-based size (text, buttons, our layout) follows the UI scale
        th.font_size = px(16.0 * scale);
        th.mono_font_size = px(13.0 * scale);
        let k = &mut th.colors;
        let set = |slot: &mut Hsla, v: Option<Hsla>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut k.background, bg);
        set(&mut k.foreground, fg);
        set(&mut k.muted_foreground, muted);
        set(&mut k.border, border);
        set(&mut k.input, border);
        set(&mut k.sidebar, bg2);
        set(&mut k.sidebar_foreground, fg);
        set(&mut k.sidebar_border, border);
        set(&mut k.title_bar, bg2);
        set(&mut k.title_bar_border, border);
        set(&mut k.status_bar, bg);
        set(&mut k.tab_bar, bg2);
        set(&mut k.tab, bg2);
        set(&mut k.tab_active, tab_active.or(bg));
        set(&mut k.tab_active_foreground, fg);
        set(&mut k.tab_foreground, muted);
        set(&mut k.popover, bg);
        set(&mut k.popover_foreground, fg);
        set(&mut k.primary, accent);
        set(&mut k.primary_hover, accent_hover.or(accent));
        set(&mut k.primary_active, accent_hover.or(accent));
        set(&mut k.primary_foreground, on_accent);
        set(&mut k.ring, accent);
        set(&mut k.caret, fg);
        set(&mut k.selection, sel);
        set(&mut k.link, link);
        set(&mut k.link_hover, link);
        set(&mut k.accent, hover);
        set(&mut k.accent_foreground, fg);
        set(&mut k.list_hover, hover);
        set(&mut k.list_active, hover);
        set(&mut k.muted, bg2);
        set(&mut k.secondary, bg2);
        set(&mut k.secondary_foreground, fg);
        set(&mut k.switch, faint);
        // the slider's track and its handle's hover ring in one colour (a band of the old
        // track colour showed as a gap between handle and ring)
        set(&mut k.slider_bar, accent);
        set(&mut k.slider_thumb, Some(gpui_kit::white()));
    });
    cx.refresh_windows();
}

thread_local! {
    /// `text_style` results by (theme generation, dark, classes): it runs on every frame.
    static STYLES: std::cell::RefCell<HashMap<(u64, bool, Vec<String>), (TextViewStyle, f32, Hsla)>> = Default::default();
}

/// The editor's text style for a note with `classes` (`cssclasses`): headings, links, code,
/// quotes, tables. `body` = the body font size in px.
pub fn text_style(cx: &App, classes: &[String]) -> (TextViewStyle, f32, Hsla) {
    use gpui_kit::component::ActiveTheme as _;
    let key = (generation(cx) ^ ((ui_scale(cx) * 1000.0) as u64) << 40, cx.theme().is_dark(), classes.to_vec());
    if let Some(hit) = STYLES.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let v = compute_text_style(cx, classes);
    STYLES.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 64 {
            c.clear();
        }
        c.insert(key, v.clone());
    });
    v
}

fn compute_text_style(cx: &App, classes: &[String]) -> (TextViewStyle, f32, Hsla) {
    use gpui_kit::component::ActiveTheme as _;
    let base = TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx));
    let dark = cx.theme().is_dark();
    let Some(t) = cx.try_global::<ThemeCss>() else { return (base, 16.0, cx.theme().yellow.opacity(0.35)) };
    let scale = ui_scale(cx);
    let body = t.length("--font-text-size", dark, classes, 16.0).unwrap_or(16.0).clamp(8.0, 40.0) * scale;
    let c = |n: &str| t.color(n, dark, classes);
    let mut s = base;
    if let Some(v) = c("--text-normal") {
        s = s.with_foreground(v);
    }
    if let Some(v) = c("--text-muted") {
        s = s.with_muted_foreground(v);
    }
    if let Some(v) = c("--link-color") {
        s = s.with_link(v);
    }
    if let Some(v) = c("--code-background") {
        s = s.with_code_background(v);
    }
    // headings: sizes (em of the body), colours, weights
    let heads: Vec<(Option<f32>, Option<Hsla>, Option<f32>)> = (1..=6)
        .map(|l| {
            let size = t.length(&format!("--h{l}-size"), dark, classes, body);
            let color = c(&format!("--h{l}-color"));
            let weight = t.var(&format!("--h{l}-weight"), dark, classes).and_then(|w| w.parse::<f32>().ok());
            (size, color, weight)
        })
        .collect();
    s = s.with_heading(move |level| {
        let mut r = StyleRefinement::default();
        if let Some((size, color, weight)) = heads.get(level.clamp(1, 6) as usize - 1) {
            if let Some(px_) = size {
                r.text.font_size = Some(px(*px_).into());
            }
            if let Some(c) = color {
                r.text.color = Some(*c);
            }
            if let Some(w) = weight {
                r.text.font_weight = Some(FontWeight(*w));
            }
        }
        r
    });
    // blockquotes
    if let Some(v) = c("--blockquote-border-color") {
        let mut r = StyleRefinement::default();
        r.border_color = Some(v);
        s = s.with_blockquote_style(r);
    }
    // tables (`.NoBorder` sets the widths to 0)
    let width = t.length("--table-border-width", dark, classes, body).unwrap_or(1.0);
    let border = if width <= 0.0 { gpui_kit::transparent_black() } else { c("--table-border-color").unwrap_or(s.border()) };
    let w: AbsoluteLength = px(width.max(0.0)).into();
    let mut table = StyleRefinement::default();
    table.border_widths = EdgesRefinement { top: Some(w), right: Some(w), bottom: Some(w), left: Some(w) };
    table.border_color = Some(border);
    s = s.with_table(table).with_border(border);
    let mut cell = StyleRefinement::default();
    cell.border_color = Some(border);
    if t.var("--table-white-space", dark, classes).as_deref() == Some("nowrap") {
        cell.text.white_space = Some(WhiteSpace::Nowrap);
    }
    s = s.with_table_cell(cell);
    if let Some(bg) = c("--table-header-background") {
        let mut head = StyleRefinement::default();
        head.background = Some(bg.into());
        s = s.with_table_head(head);
    }
    let highlight = c("--text-highlight-bg").unwrap_or(cx.theme().yellow.opacity(0.35));
    (s, body, highlight)
}

/// Colour and thickness of `---` rules.
pub fn hr(cx: &App, classes: &[String]) -> (Hsla, f32) {
    use gpui_kit::component::ActiveTheme as _;
    let dark = cx.theme().is_dark();
    let t = cx.try_global::<ThemeCss>();
    let color = t.and_then(|t| t.color("--hr-color", dark, classes)).unwrap_or(cx.theme().border);
    let w = t.and_then(|t| t.length("--hr-thickness", dark, classes, 16.0)).unwrap_or(2.0);
    (color, w)
}

/// The UI scale (Settings → Appearance), 1.0 = Obsidian's sizes at 100% zoom.
pub fn ui_scale(cx: &App) -> f32 {
    crate::app_settings::AppSettings::value(cx, "uiScale").and_then(|v| v.as_f64()).map(|v| v as f32).filter(|v| (0.5..=3.0).contains(v)).unwrap_or(1.0)
}

pub fn generation(cx: &App) -> u64 {
    cx.try_global::<ThemeCss>().map_or(0, |t| t.generation)
}

#[cfg(test)]
mod tests {
    use super::{classify, Mode, Scope, ThemeCss};

    #[test]
    fn selectors() {
        assert_eq!(classify(":root"), Scope::Global(Mode::Both));
        assert_eq!(classify("body.theme-dark"), Scope::Global(Mode::Dark));
        assert_eq!(classify(".theme-light"), Scope::Global(Mode::Light));
        assert_eq!(classify("body:not(.theme-dark)"), Scope::Global(Mode::Light));
        assert_eq!(classify(".NoBorder"), Scope::Class("NoBorder".into(), Mode::Both));
        assert_eq!(classify(".markdown-preview-view.NoBorder table"), Scope::Class("NoBorder".into(), Mode::Both));
        assert_eq!(classify(".theme-dark .callout[data-callout=\"thm\"]"), Scope::Callout("thm".into(), Mode::Dark));
        assert_eq!(classify("h1"), Scope::Other);
        assert_eq!(classify(".markdown-preview-view h1"), Scope::Other);
        assert_eq!(classify(".a.b"), Scope::Other);
    }

    #[test]
    fn layers_classes_and_reports() {
        let dir = std::env::temp_dir().join(format!("ol-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".obsidian/snippets")).unwrap();
        std::fs::create_dir_all(dir.join(".obsidian/themes/Mine")).unwrap();
        std::fs::write(dir.join(".obsidian/themes/Mine/theme.css"), ".theme-dark { --accent-h: 120; } body { --h1-color: red; } .markdown-preview-view h1 { text-transform: uppercase; } @font-face { font-family: X; }").unwrap();
        std::fs::write(dir.join(".obsidian/snippets/table_border.css"), ".NoBorder {\n--table-border-width: 0px;\n--table-white-space: normal;\n}").unwrap();
        std::fs::write(dir.join(".obsidian/snippets/callouts.css"), ".callout[data-callout=\"thm\"] { --callout-color: 0, 120, 200; } :root { --font-text-size: 18px; }").unwrap();
        // the callout snippet off: its global and callout rules don't apply; the class rule always does
        let t = ThemeCss::load(&dir, "Mine", &[], 1);
        let none: Vec<String> = Vec::new();
        assert_eq!(t.var("--h1-color", false, &none).as_deref(), Some("red"));
        assert_eq!(t.var("--interactive-accent", true, &none).as_deref(), Some("hsl(120, 88%, 66%)"), "the theme's dark accent hue");
        assert_eq!(t.var("--interactive-accent", false, &none).as_deref(), Some("hsl(258, 88%, 66%)"));
        assert_eq!(t.length("--table-border-width", false, &none, 16.0), Some(1.0));
        assert_eq!(t.length("--table-border-width", false, &["NoBorder".to_string()], 16.0), Some(0.0));
        assert_eq!(t.length("--font-text-size", false, &none, 16.0), Some(16.0));
        assert!(t.callout_color("thm", false).is_none());
        let theme = t.reports.iter().find(|r| r.is_theme).unwrap();
        assert_eq!((theme.global_vars, theme.ignored), (2, 2));
        assert!(theme.examples.iter().any(|e| e.contains("text-transform")));
        let tb = t.reports.iter().find(|r| r.name == "table_border").unwrap();
        assert_eq!((tb.class_vars, tb.ignored, tb.classes.clone()), (2, 0, vec!["NoBorder".to_string()]));
        // enabled
        let t = ThemeCss::load(&dir, "", &["callouts".to_string()], 2);
        assert_eq!(t.length("--font-text-size", false, &none, 16.0), Some(18.0));
        assert!(t.callout_color("thm", true).is_some());
        assert_eq!(t.var("--h1-color", false, &none), None, "no theme: inherit");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
