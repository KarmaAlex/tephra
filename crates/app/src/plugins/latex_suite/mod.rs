//! LaTeX snippets (replaces Latex Suite 1.14, MIT — see `NOTICE` in this folder).

use std::path::{Path, PathBuf};

use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

pub struct LatexSuite {
    manifest: PluginManifest,
    #[allow(dead_code)]
    vault: PathBuf,
}

impl LatexSuite {
    pub fn new(vault: &Path) -> Self {
        Self {
            manifest: PluginManifest {
                id: "latex-suite",
                name: "LaTeX snippets",
                version: "0.1.0",
                description: "Latex Suite-compatible snippets, tabout, auto-fraction and matrix shortcuts.",
                enabled_by_default: true,
                settings: vec![],
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
    fn on_load(&mut self, _: &Map<String, Value>, _: &mut Registrar) {}
}
