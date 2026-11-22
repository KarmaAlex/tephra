//! Core (non-plugin) settings, saved in `<vault>/.obsidilight/app.json`. Unknown keys are kept.

use std::path::PathBuf;

use gpui_kit::*;
use serde_json::{Map, Value};

pub struct AppSettings {
    values: Map<String, Value>,
    path: Option<PathBuf>,
}

impl Global for AppSettings {}

/// (key, label, description, default) of every boolean core setting.
pub const TOGGLES: &[(&str, &str, &str, bool)] = &[
    ("autoPairBrackets", "Auto-pair brackets", "Typing ( [ { \" or ` also types the closing half, with the caret in between.", true),
    ("centerDisplayMath", "Centre display math", "Draw $$…$$ formulas centred, as Obsidian does (off: aligned left).", false),
];

impl AppSettings {
    pub fn init(cx: &mut App, vault_root: &std::path::Path) {
        let path = vault_root.join(".obsidilight/app.json");
        let values = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default();
        cx.set_global(AppSettings { values, path: Some(path) });
    }

    pub fn flag(cx: &App, key: &str) -> bool {
        let default = TOGGLES.iter().find(|t| t.0 == key).is_some_and(|t| t.3);
        cx.try_global::<AppSettings>().and_then(|s| s.values.get(key)).and_then(Value::as_bool).unwrap_or(default)
    }

    pub fn set(cx: &mut App, key: &str, value: Value) {
        let s = cx.global_mut::<AppSettings>();
        s.values.insert(key.to_string(), value);
        if let Some(p) = &s.path {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let tmp = p.with_extension("json.obsidilight-tmp");
            if std::fs::write(&tmp, serde_json::to_string_pretty(&s.values).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(&tmp, p);
            }
        }
        cx.refresh_windows();
    }
}
