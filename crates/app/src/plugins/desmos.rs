//! Desmos: not built yet (HANDOFF §5.3 "don't build yet"). The slot exists so the toggle and the
//! `desmos-graph` code-block hook are in place; blocks show as code with a note.

use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

pub struct Desmos {
    manifest: PluginManifest,
}

impl Desmos {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "desmos",
                name: "Desmos",
                version: "0.0.0",
                description: "Graphs from `desmos-graph` code blocks. Not built yet: blocks show as code.",
                enabled_by_default: false,
                settings: vec![],
                code_languages: &["desmos-graph"],
            },
        }
    }
}

impl Plugin for Desmos {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, _: &mut Registrar) {}
}
