//! Page preview (Obsidian core plugin): hovering an internal link shows the note (or the
//! heading's section, or the block) in a popup. As in Obsidian, the editor needs Ctrl held,
//! reading view doesn't; sidebar lists (files, backlinks, search, bookmarks) and the graph
//! have their own choice. Academic Notes adds plain-hover previews of references.

use std::rc::Rc;

use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

pub struct PagePreview {
    manifest: PluginManifest,
}

impl PagePreview {
    pub fn new() -> Self {
        let toggle = |key, label, description, on: bool| SettingSpec { key, label, description, kind: SettingKind::Toggle, default: Value::Bool(on) };
        Self {
            manifest: PluginManifest {
                id: "page-preview",
                name: "Page preview",
                version: "0.1.0",
                description: "Preview a linked note by hovering the link.",
                enabled_by_default: true,
                settings: vec![
                    toggle("ctrlEditor", "Editor: require Ctrl", "In live preview and source mode, show previews only while Ctrl is held.", true),
                    toggle("ctrlReading", "Reading view: require Ctrl", "In reading view, show previews only while Ctrl is held.", false),
                    toggle("ctrlSidebar", "Sidebars and graph: require Ctrl", "In the file list, backlinks, search results, bookmarks and the graph.", true),
                ],
                code_languages: &[],
            },
        }
    }
}

struct Policy {
    ctrl_editor: bool,
    ctrl_reading: bool,
    ctrl_sidebar: bool,
}

impl HoverPolicy for Policy {
    fn wants(&self, _target: &str, source: HoverSource, ctrl: bool) -> bool {
        ctrl || !match source {
            HoverSource::Editor => self.ctrl_editor,
            HoverSource::Reading => self.ctrl_reading,
            HoverSource::Sidebar => self.ctrl_sidebar,
        }
    }
}

impl Plugin for PagePreview {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let flag = |k: &str, d: bool| settings.get(k).and_then(Value::as_bool).unwrap_or(d);
        reg.hover_policy(Rc::new(Policy { ctrl_editor: flag("ctrlEditor", true), ctrl_reading: flag("ctrlReading", false), ctrl_sidebar: flag("ctrlSidebar", true) }));
    }
}

/// Hover previews for a sidebar row showing a vault note (`sub` = heading / `^block`, or "").
pub trait PreviewOnHover: Sized {
    fn preview_on_hover(self, ws: &WorkspaceHandle, rel: &str, sub: &str) -> Self;
}

impl PreviewOnHover for Stateful<Div> {
    fn preview_on_hover(self, ws: &WorkspaceHandle, rel: &str, sub: &str) -> Self {
        let (ws1, ws2) = (ws.clone(), ws.clone());
        let (rel, sub) = (rel.to_string(), sub.to_string());
        let target = if sub.is_empty() { rel.clone() } else { format!("{rel}#{sub}") };
        self.on_mouse_move(move |ev: &MouseMoveEvent, _, cx| {
            let ctrl = ev.modifiers.secondary(cx);
            let want = super::PluginHost::wants_preview(cx, &target, HoverSource::Sidebar, ctrl);
            ws1.preview_link(want.then(|| (rel.clone(), sub.clone())), ev.position, cx);
        })
        .on_hover(move |hovered: &bool, window, cx| {
            if !*hovered {
                ws2.preview_link(None, window.mouse_position(), cx);
            }
        })
    }
}
