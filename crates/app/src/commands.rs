//! The app's own commands (not a plugin, never unloaded): workspace and editor actions, so
//! the command palette lists them next to plugin commands. Their hotkeys are the app's key
//! bindings (registered in `workspace::bind_keys` / `editor::bind_keys`), shown in the palette.

use std::rc::Rc;

use gpui_kit::*;

use crate::editor::ViewMode;
use crate::plugins::api::{Action, ActionCtx, Command};
use crate::plugins::PluginHost;
use crate::workspace::Workspace;

/// Plugin id under which the app's commands are registered.
pub const APP: &str = "app";

fn ws_cmd(f: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) -> Action {
    Rc::new(move |a: &mut ActionCtx| {
        if let Some(h) = a.workspace() {
            let _ = h.0.update(a.cx, |ws, cx| f(ws, a.window, cx));
        }
    })
}

fn editor_cmd(f: impl Fn(&mut crate::editor::Editor, &mut Context<crate::editor::Editor>) + 'static) -> Action {
    Rc::new(move |a: &mut ActionCtx| {
        if let Some(e) = a.editor.clone() {
            e.update(a.cx, |e, cx| f(e, cx));
        }
    })
}

pub fn register_core(cx: &mut App) {
    let cmds: Vec<(&str, &str, Option<&str>, Action)> = vec![
        ("new-note", "Create new note", Some("ctrl-n"), ws_cmd(|ws, w, cx| ws.new_note_in("", w, cx))),
        ("close-tab", "Close current tab", Some("ctrl-w"), ws_cmd(|ws, w, cx| ws.close_tab(ws.active, w, cx))),
        ("next-tab", "Go to next tab", Some("ctrl-tab"), ws_cmd(|ws, w, cx| ws.activate((ws.active + 1) % ws.tabs.len().max(1), w, cx))),
        ("prev-tab", "Go to previous tab", Some("ctrl-shift-tab"), ws_cmd(|ws, w, cx| ws.activate((ws.active + ws.tabs.len().max(1) - 1) % ws.tabs.len().max(1), w, cx))),
        ("toggle-left-sidebar", "Toggle left sidebar", Some("ctrl-shift-b"), ws_cmd(|ws, _, cx| ws.toggle_sidebar(true, cx))),
        ("toggle-right-sidebar", "Toggle right sidebar", None, ws_cmd(|ws, _, cx| ws.toggle_sidebar(false, cx))),
        ("reveal-file", "Reveal current file in navigation", None, ws_cmd(|ws, _, cx| ws.reveal_active(cx))),
        ("rename-file", "Rename current file", Some("f2"), ws_cmd(|ws, w, cx| ws.rename_active(w, cx))),
        ("delete-file", "Delete current file", None, ws_cmd(|ws, w, cx| ws.delete_active(w, cx))),
        ("open-settings", "Open settings", Some("ctrl-,"), Rc::new(|a: &mut ActionCtx| crate::settings_ui::open(a.window, a.cx))),
        ("save", "Save current file", Some("ctrl-s"), editor_cmd(|e, cx| e.save(cx))),
        ("toggle-reading", "Toggle reading view", Some("ctrl-e"), editor_cmd(|e, cx| e.toggle_mode(ViewMode::Reading, cx))),
        ("toggle-source", "Toggle source mode", Some("ctrl-shift-e"), editor_cmd(|e, cx| e.toggle_mode(ViewMode::Source, cx))),
    ];
    let host = cx.global_mut::<PluginHost>();
    host.register_core(APP, |r| {
        for (id, name, hotkey, run) in cmds {
            r.command(Command { id: id.into(), name: name.into(), hotkey: hotkey.map(String::from), run });
        }
    });
}
