//! LaTeX snippets (replaces Latex Suite 1.14, MIT — see `NOTICE` in this folder): snippets from
//! the vault's snippets file (Latex Suite syntax, function snippets run in QuickJS), tabstops,
//! tabout, auto-fraction, matrix shortcuts, auto-enlarge brackets, auto-delete `$`.
//!
//! Default settings come from `.obsidian/plugins/obsidian-latex-suite/data.json` (read only);
//! changes are saved in `.obsidilight/plugins/latex-suite.json`.

pub mod context;
pub mod features;
pub mod js;
pub mod regex_js;
pub mod snippets;
pub mod tex;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use obsidilight_core::EditKind;
use serde_json::{Map, Value};

use super::api::*;
use crate::editor::EditorApi;
use context::{default_mathless_macros, Ctx, MacroArgs};
use snippets::{Insert, Replacement, Snippet, Trigger};

static DEFAULT_SNIPPETS: &str = include_str!("default_snippets.js");
static DEFAULT_VARIABLES: &str = include_str!("default_snippet_variables.js");

/// Everything the engine reads from the plugin's settings.
#[derive(Clone, Debug)]
pub struct Settings {
    pub snippets_enabled: bool,
    pub remove_whitespace: bool,
    pub auto_delete_dollar: bool,
    pub autofraction: bool,
    pub autofraction_symbol: String,
    pub autofraction_breaking: String,
    pub autofraction_excluded: Vec<(String, String)>,
    pub matrix_shortcuts: bool,
    pub matrix_envs: Vec<String>,
    pub matrix_macros: Vec<String>,
    pub tabout: bool,
    pub tabout_exit_on_eol: bool,
    pub tabout_closing: Vec<String>,
    pub enlarge_brackets: bool,
    pub enlarge_space: bool,
    pub enlarge_triggers: Vec<String>,
    pub word_delimiters: String,
    pub recursion: usize,
    pub force_math_languages: Vec<String>,
    pub mathless: Vec<MacroArgs>,
}

fn list(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

impl Settings {
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let b = |k: &str, d: bool| m.get(k).and_then(Value::as_bool).unwrap_or(d);
        let s = |k: &str, d: &str| m.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let excluded: Vec<(String, String)> = serde_json::from_str::<Vec<(String, String)>>(&s("autofractionExcludedEnvs", r#"[["^{", "}"], ["\\pu{", "}"]]"#)).unwrap_or_default();
        let mut mathless = default_mathless_macros();
        if let Ok(Value::Object(tm)) = serde_json::from_str::<Value>(&s("textMacros", "{}")) {
            for (kind, restricted) in [("text", false), ("restricted", true)] {
                for item in tm.get(kind).and_then(Value::as_array).into_iter().flatten() {
                    match item {
                        Value::String(n) => mathless.push(MacroArgs { name: n.clone(), args: Some(vec![0]), restricted }),
                        Value::Object(o) => mathless.push(MacroArgs {
                            name: o.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            args: o.get("arguments").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()),
                            restricted,
                        }),
                        _ => {}
                    }
                }
            }
        }
        Settings {
            snippets_enabled: b("snippetsEnabled", true),
            remove_whitespace: b("removeSnippetWhitespace", true),
            auto_delete_dollar: b("autoDelete$", true),
            autofraction: b("autofractionEnabled", true),
            autofraction_symbol: s("autofractionSymbol", "\\frac"),
            autofraction_breaking: s("autofractionBreakingChars", "+-=\t"),
            autofraction_excluded: excluded,
            matrix_shortcuts: b("matrixShortcutsEnabled", true),
            matrix_envs: list(&s("matrixShortcutsEnvNames", "pmatrix, cases, align, gather, bmatrix, Bmatrix, vmatrix, Vmatrix, array, matrix, aligned")),
            matrix_macros: list(&s("matrixShortcutsMacroNames", "eqalign")),
            tabout: b("taboutEnabled", true),
            tabout_exit_on_eol: b("taboutExitEquationOnlyOnEOL", true),
            tabout_closing: list(&s("taboutClosingSymbols", "), ], \\rbrack, \\}, \\rbrace, \\rangle, \\rvert, \\rVert, \\rfloor, \\rceil, \\urcorner, }")),
            enlarge_brackets: b("autoEnlargeBrackets", true),
            enlarge_space: b("autoEnlargeBracketsSpace", true),
            enlarge_triggers: list(&s("autoEnlargeBracketsTriggers", "sum, int, frac, prod, bigcup, bigcap")),
            word_delimiters: s("wordDelimiters", "., +-\\n\t:;!?\\/{}[]()=~$'\"|`<>*^%#@&").replace("\\n", "\n"),
            recursion: m.get("snippetRecursion").and_then(Value::as_u64).unwrap_or(0) as usize,
            force_math_languages: list(&s("forceMathLanguages", "math")),
            mathless,
        }
    }
}

/// Inserts an expanded replacement over `start..to`, Latex Suite style: with `key`, the key is
/// first typed as its own undo step (so undo restores what was typed); newlines keep the line's
/// quote marks and indentation.
pub(crate) fn apply_insert(e: &mut EditorApi, start: usize, to: usize, mut ins: Insert, key: Option<&str>, text: &str) {
    let ls = text[..start].rfind('\n').map_or(0, |i| i + 1);
    ins.text = tex::keep_indent_and_callout(&ins.text, &mut ins.tabstops, &text[ls..start]);
    // pure continuation of what is there (e.g. "keep typing this macro"): just type the rest
    if ins.tabstops.is_empty() && ins.text.starts_with(&text[start..to]) && !e.has_tabstops() {
        let rest = ins.text[to - start..].to_string();
        e.replace(to..to, &rest, EditKind::Insert);
        return;
    }
    let mut end = to;
    if let Some(k) = key {
        e.replace(to..to, k, EditKind::Other);
        end = to + k.len();
    }
    e.insert_snippet(start..end, &ins.text, &ins.tabstops, EditKind::Other);
}

pub struct Engine {
    pub settings: Settings,
    pub snippets: Vec<Snippet>,
    pub js: Option<js::Js>,
    /// Load problems (shown in the plugin's settings).
    pub errors: Vec<String>,
}

#[derive(Clone, Copy)]
enum Set<'a> {
    /// Automatic snippets, run on a typed key.
    Automatic,
    /// Snippets bound to a key: "" = the snippets trigger (Tab), else a character (visual).
    Key(&'a str),
}

impl Engine {
    /// Loads snippets and variables. `files` reads a vault-relative file.
    pub fn load(settings: &Map<String, Value>, read: &dyn Fn(&str) -> Option<String>) -> Self {
        let s = Settings::from_map(settings);
        let mut errors = Vec::new();
        let js = match js::Js::new() {
            Ok(j) => Some(j),
            Err(e) => {
                errors.push(format!("JavaScript engine: {e}"));
                None
            }
        };
        let get = |k: &str| settings.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
        let mut snippets = Vec::new();
        if let Some(js) = &js {
            let vars_src = if settings.get("loadSnippetVariablesFromFile").and_then(Value::as_bool) == Some(true) {
                read(&get("snippetVariablesFileLocation")).unwrap_or_else(|| {
                    errors.push(format!("snippet variables file not found: {}", get("snippetVariablesFileLocation")));
                    DEFAULT_VARIABLES.to_string()
                })
            } else {
                let v = get("snippetVariables");
                if v.is_empty() { DEFAULT_VARIABLES.to_string() } else { v }
            };
            let vars = js.load_variables(&vars_src).unwrap_or_else(|e| {
                errors.push(e);
                js.load_variables(DEFAULT_VARIABLES).unwrap_or_default()
            });
            let file = get("snippetsFileLocation");
            let src = if file.is_empty() {
                Some(DEFAULT_SNIPPETS.to_string())
            } else {
                let r = read(&file);
                if r.is_none() {
                    errors.push(format!("snippets file not found: {file}"));
                }
                r
            };
            if let Some(src) = src {
                match js.load_snippets(&src) {
                    Ok(descs) => {
                        let (sn, warnings) = snippets::build(&descs, &vars);
                        snippets = sn;
                        errors.extend(warnings);
                    }
                    Err(e) => errors.push(e),
                }
            }
        }
        for e in &errors {
            eprintln!("latex-suite: {e}");
        }
        Engine { settings: s, snippets, js, errors }
    }

    fn ctx(&self, text: &str, pos: usize) -> Ctx {
        Ctx::new(text, pos, &self.settings.force_math_languages, &self.settings.mathless)
    }

    /// Runs one set of snippets at the caret (Latex Suite's `runSnippets`).
    fn run_snippets(&self, e: &mut EditorApi, key: Option<&str>, set: Set) -> bool {
        if !self.settings.snippets_enabled {
            return false;
        }
        let mut key = key;
        let mut did = false;
        for _ in 0..=self.settings.recursion {
            if !self.run_once(e, key, set) {
                break;
            }
            did = true;
            key = None;
        }
        did
    }

    fn run_once(&self, e: &mut EditorApi, key: Option<&str>, set: Set) -> bool {
        let text = e.text();
        let sel = e.selection().range();
        let to = sel.end;
        let ctx = self.ctx(&text, to);
        let k = key.unwrap_or("");
        let mut line = String::with_capacity(to + k.len());
        line.push_str(&text[..to]);
        line.push_str(k);
        // regexes search from the previous line on (earlier text only gives context)
        let ls = text[..to].rfind('\n').map_or(0, |i| i + 1);
        let mut window = if ls == 0 { 0 } else { text[..ls - 1].rfind('\n').map_or(0, |i| i + 1) };
        window = window.max(to.saturating_sub(4000));
        while !line.is_char_boundary(window) {
            window += 1;
        }
        let selected = (!sel.is_empty()).then(|| text[sel.clone()].to_string());
        for sn in &self.snippets {
            let wanted = match set {
                Set::Automatic => sn.automatic,
                Set::Key("") => !sn.automatic && (sn.trigger_key.is_empty() || sn.trigger_key == "Tab"),
                Set::Key(c) => sn.trigger_key == c,
            };
            if !wanted || !sn.mode.runs_in(&ctx.mode) {
                continue;
            }
            let visual = matches!(sn.trigger, Trigger::Visual);
            let (start, groups) = if visual {
                if selected.is_none() {
                    continue;
                }
                (sel.start, Vec::new())
            } else {
                if selected.is_some() {
                    continue;
                }
                match sn.match_at(&line, window, to, self.js.as_ref()) {
                    Some(m) => (m.start, m.groups),
                    None => continue,
                }
            };
            if !sn.excluded_envs.is_empty() && ctx.envs.iter().any(|en| matches!(en, context::EnvName::Environment { name, .. } if sn.excluded_envs.contains(name))) {
                continue;
            }
            if sn.word_boundary && !snippets::on_word_boundary(&text, start, to, &self.settings.word_delimiters) {
                continue;
            }
            let captures: Vec<String> = groups.iter().skip(1).map(|g| g.clone().unwrap_or_default()).collect();
            let mut ins = match &sn.replacement {
                Replacement::Text(t) => snippets::expand(t, &captures, selected.as_deref()),
                Replacement::Js { index, needs_state } => {
                    let Some(js) = &self.js else { continue };
                    let doc = Rc::new(text.clone());
                    let (from16, to16) = (snippets::byte_to_utf16(&text, sel.start), snippets::byte_to_utf16(&text, sel.end));
                    let arg = if visual {
                        js::CallArg::Visual(selected.as_deref().unwrap_or(""))
                    } else if sn.is_regex() {
                        js::CallArg::Regex { groups: &groups, index: snippets::byte_to_utf16(&line, start), input: &line }
                    } else {
                        js::CallArg::Str(match &sn.trigger {
                            Trigger::Str(t) => t,
                            _ => "",
                        })
                    };
                    match js.call(*index, arg, from16, to16, doc, *needs_state) {
                        Ok(Some(r)) => snippets::expand_tabstops(&r),
                        Ok(None) => continue,
                        Err(err) => {
                            eprintln!("latex-suite: snippet \"{}\" failed: {err}", sn.description);
                            continue;
                        }
                    }
                }
            };
            if visual && ins.tabstops.is_empty() {
                ins.tabstops.push((0, 0..ins.text.len()));
            }
            if ctx.mode.inline_math && self.settings.remove_whitespace {
                snippets::trim_end(&mut ins);
            }
            let enlarge = self.settings.enlarge_triggers.iter().any(|w| ins.text.contains(w.as_str()));
            let undo_key = (sn.automatic && !visual && sn.undo_key).then_some(k).filter(|k| !k.is_empty());
            let end = if visual { sel.end } else { to };
            apply_insert(e, start, end, ins, undo_key, &text);
            if enlarge {
                features::enlarge_brackets(e, &self.settings);
            }
            return true;
        }
        false
    }

    /// A typed character (before it is inserted).
    pub fn on_char(&self, e: &mut EditorApi, ch: &str) -> bool {
        if self.run_snippets(e, Some(ch), Set::Automatic) {
            return true;
        }
        if self.run_snippets(e, None, Set::Key(ch)) {
            return true;
        }
        let text = e.text();
        let ctx = self.ctx(&text, e.selection().range().end);
        if ch == "/" && features::autofraction(e, &self.settings, &ctx, &text) {
            return true;
        }
        if self.settings.tabout && features::tabout_by_close_bracket(e, &self.settings, &ctx, &text, ch) {
            return true;
        }
        false
    }

    pub fn on_key(&self, e: &mut EditorApi, key: EditorKey) -> bool {
        let s = &self.settings;
        match key {
            EditorKey::Tab => {
                if self.run_snippets(e, None, Set::Key("")) || e.next_tabstop(false) {
                    return true;
                }
                let text = e.text();
                let ctx = self.ctx(&text, e.selection().range().end);
                features::matrix_priority_tabout(e, s, &ctx, &text) || features::matrix_cell(e, s, &ctx) || (s.tabout && features::tabout(e, s, &ctx, &text))
            }
            EditorKey::ShiftTab => {
                if e.next_tabstop(true) {
                    return true;
                }
                let text = e.text();
                let ctx = self.ctx(&text, e.selection().range().end);
                s.tabout && features::reverse_tabout(e, s, &ctx, &text)
            }
            EditorKey::Enter | EditorKey::ShiftEnter => {
                let text = e.text();
                let ctx = self.ctx(&text, e.selection().range().end);
                if key == EditorKey::Enter { features::matrix_newline(e, s, &ctx, &text) } else { features::matrix_exit(e, s, &ctx, &text) }
            }
            EditorKey::Backspace => {
                let text = e.text();
                features::auto_delete_dollar(e, s, &text)
            }
        }
    }
}

struct Hook(Engine);

impl EditorHook for Hook {
    fn on_char(&self, e: &mut EditorApi, ch: &str) -> bool {
        self.0.on_char(e, ch)
    }
    fn on_key(&self, e: &mut EditorApi, key: EditorKey) -> bool {
        self.0.on_key(e, key)
    }
}

pub struct LatexSuite {
    manifest: PluginManifest,
    vault: PathBuf,
}

impl LatexSuite {
    pub fn new(vault: &Path) -> Self {
        // the vault's Latex Suite settings are the defaults (read only)
        let ls: Map<String, Value> = std::fs::read_to_string(vault.join(".obsidian/plugins/obsidian-latex-suite/data.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let b = |k: &str, d: bool| ls.get(k).and_then(Value::as_bool).unwrap_or(d);
        let st = |k: &str, d: &str| ls.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let file = if b("loadSnippetsFromFile", false) { st("snippetsFileLocation", "").trim().to_string() } else { String::new() };
        let toggle = |key: &'static str, label: &'static str, description: &'static str, d: bool| SettingSpec { key, label, description, kind: SettingKind::Toggle, default: Value::Bool(b(key, d)) };
        let text = |key: &'static str, label: &'static str, description: &'static str, d: &str| SettingSpec { key, label, description, kind: SettingKind::Text, default: Value::from(st(key, d)) };
        let settings = vec![
            SettingSpec { key: "snippetsFileLocation", label: "Snippets file", description: "Vault-relative path of a Latex Suite snippets file (empty = built-in defaults).", kind: SettingKind::Text, default: Value::from(file) },
            toggle("snippetsEnabled", "Snippets", "Expand snippets while typing.", true),
            toggle("removeSnippetWhitespace", "Trim snippet whitespace in inline math", "Remove trailing spaces of expansions inside $…$.", true),
            toggle("autoDelete$", "Auto-delete $", "Backspace in an empty $|$ (or ${}|{}$) removes the whole formula.", true),
            toggle("autofractionEnabled", "Auto-fraction", "Typing / in math turns the previous term into a fraction.", true),
            text("autofractionSymbol", "Fraction command", "Used by auto-fraction.", "\\frac"),
            text("autofractionBreakingChars", "Fraction breaking characters", "Characters that end the numerator.", "+-=\t"),
            toggle("matrixShortcutsEnabled", "Matrix shortcuts", "Tab adds a cell, Enter a row, Shift+Enter leaves the environment.", true),
            toggle("taboutEnabled", "Tabout", "Tab moves past the next closing bracket or out of the equation.", true),
            toggle("taboutExitEquationOnlyOnEOL", "Leave equations only at the end", "Tabout leaves an equation only from its end.", true),
            toggle("autoEnlargeBrackets", "Auto-enlarge brackets", "Brackets around \\sum, \\frac… become \\left( \\right).", true),
            toggle("autoEnlargeBracketsSpace", "Space inside enlarged brackets", "", true),
            text("wordDelimiters", "Word delimiters", "Characters that delimit words for snippets with the w option.", "., +-\\n\t:;!?\\/{}[]()=~$'\"|`<>*^%#@&"),
            text("matrixShortcutsEnvNames", "Matrix environments", "Comma-separated.", "pmatrix, cases, align, gather, bmatrix, Bmatrix, vmatrix, Vmatrix, array, matrix, aligned"),
            text("taboutClosingSymbols", "Tabout closing symbols", "Comma-separated.", "), ], \\rbrack, \\}, \\rbrace, \\rangle, \\rvert, \\rVert, \\rfloor, \\rceil, \\urcorner, }"),
            text("autoEnlargeBracketsTriggers", "Auto-enlarge triggers", "Comma-separated.", "sum, int, frac, prod, bigcup, bigcap"),
        ];
        // advanced values without their own control: kept as imported
        let mut hidden = Vec::new();
        for k in ["autofractionExcludedEnvs", "matrixShortcutsMacroNames", "forceMathLanguages", "snippetVariables", "snippetVariablesFileLocation", "textMacros"] {
            if let Some(v) = ls.get(k) {
                hidden.push(SettingSpec { key: k, label: k, description: "", kind: SettingKind::Hidden, default: v.clone() });
            }
        }
        for k in ["snippetRecursion", "loadSnippetVariablesFromFile"] {
            if let Some(v) = ls.get(k) {
                hidden.push(SettingSpec { key: k, label: k, description: "", kind: SettingKind::Hidden, default: v.clone() });
            }
        }
        let mut all = settings;
        all.extend(hidden);
        Self {
            manifest: PluginManifest {
                id: "latex-suite",
                name: "LaTeX snippets",
                version: "0.1.0",
                description: "Latex Suite-compatible snippets, tabstops, tabout, auto-fraction, matrix shortcuts and auto-enlarged brackets.",
                enabled_by_default: true,
                settings: all,
                code_languages: &[],
            },
            vault: vault.to_path_buf(),
        }
    }
}

impl Plugin for LatexSuite {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let vault = self.vault.clone();
        let read = move |rel: &str| -> Option<String> {
            let p = vault.join(rel);
            // stay inside the vault
            let canon = std::fs::canonicalize(&p).ok()?;
            let root = std::fs::canonicalize(&vault).ok()?;
            canon.starts_with(&root).then(|| std::fs::read_to_string(canon).ok()).flatten()
        };
        let engine = Engine::load(settings, &read);
        let summary = format!("{} snippets loaded.", engine.snippets.len());
        let errors = engine.errors.clone();
        reg.settings_tab(SettingsTab {
            id: "status".into(),
            title: "Snippets status".into(),
            render: Rc::new(move |_, _| {
                use gpui_kit::{div, IntoElement as _, ParentElement as _, Styled as _};
                div().flex().flex_col().gap_1().text_sm().child(summary.clone()).children(errors.iter().map(|e| div().text_color(gpui_kit::red()).child(e.clone()))).into_any_element()
            }),
        });
        reg.editor_hook(Rc::new(Hook(engine)));
    }
}

#[cfg(test)]
mod tests;
