//! Command palette (Obsidian core plugin, Ctrl+P): every command of the app and of the enabled
//! plugins, by name, with its hotkey. Recently used commands come first.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::App;
use serde_json::{Map, Value};

use super::api::*;
use super::picker::{self, hotkey_label, PickItem};
use super::PluginHost;
use crate::fuzzy;

/// (command id, shown name "Plugin: Command", hotkey) of every registered command.
fn all_commands(cx: &App) -> Vec<(String, String, Option<String>, Action)> {
    let Some(h) = cx.try_global::<PluginHost>() else { return Vec::new() };
    let mut out: Vec<(String, String, Option<String>, Action)> = h
        .reg
        .commands
        .iter()
        .map(|(p, c)| {
            let name = match h.plugin_name(p) {
                Some(pn) => format!("{pn}: {}", c.name),
                None => c.name.clone(),
            };
            (c.id.clone(), name, c.hotkey.clone(), c.run.clone())
        })
        .collect();
    // every sidebar view can be shown from here ("Backlinks: Show backlinks")
    for (p, v) in &h.reg.sidebar_views {
        let id = v.id.clone();
        let run: Action = Rc::new(move |a: &mut ActionCtx| {
            if let Some(ws) = a.workspace() {
                ws.show_view(&id, None, a.cx);
            }
        });
        let pn = h.plugin_name(p).unwrap_or("");
        out.push((format!("view:{}", v.id), format!("{pn}: Show {}", v.title.to_lowercase()), None, run));
    }
    out
}

/// Order for `query`: recently used first when empty, else by match quality.
pub fn rank(names: &[(String, String)], recent: &[String], query: &str) -> Vec<(usize, Vec<std::ops::Range<usize>>)> {
    let pos = |id: &str| recent.iter().position(|r| r == id).unwrap_or(usize::MAX);
    let mut v: Vec<(u32, usize, usize, Vec<std::ops::Range<usize>>)> =
        names.iter().enumerate().filter_map(|(i, (id, name))| fuzzy::score(query, name).map(|(c, hl)| (c, pos(id), i, hl))).collect();
    if query.trim().is_empty() {
        v.sort_by(|a, b| (a.1, &names[a.2].1).cmp(&(b.1, &names[b.2].1)));
    } else {
        v.sort_by(|a, b| (a.0, a.1, &names[a.2].1).cmp(&(b.0, b.1, &names[b.2].1)));
    }
    v.into_iter().map(|(_, _, i, hl)| (i, hl)).collect()
}

pub struct CommandPalette {
    manifest: PluginManifest,
    recent: Rc<RefCell<Vec<String>>>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: "command-palette",
                name: "Command palette",
                version: "0.1.0",
                description: "Run any command by name (Ctrl+P).",
                enabled_by_default: true,
                settings: vec![],
                code_languages: &[],
            },
            recent: Rc::default(),
        }
    }
}

impl Plugin for CommandPalette {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, _: &Map<String, Value>, reg: &mut Registrar) {
        let recent = self.recent.clone();
        let open: Action = Rc::new(move |a: &mut ActionCtx| {
            let shown: Rc<RefCell<Vec<(String, Action)>>> = Rc::default();
            let (r1, s1) = (recent.clone(), shown.clone());
            let items: picker::Items = Rc::new(move |q: &str, cx: &App| {
                let cmds: Vec<_> = all_commands(cx).into_iter().filter(|c| c.0 != "command-palette:open").collect();
                let names: Vec<(String, String)> = cmds.iter().map(|c| (c.0.clone(), c.1.clone())).collect();
                let order = rank(&names, &r1.borrow(), q);
                *s1.borrow_mut() = order.iter().map(|(i, _)| (cmds[*i].0.clone(), cmds[*i].3.clone())).collect();
                order.into_iter().map(|(i, hl)| PickItem { label: cmds[i].1.clone(), hl, detail: String::new(), hint: cmds[i].2.as_deref().map(hotkey_label).unwrap_or_default() }).collect()
            });
            let r2 = recent.clone();
            let pick: picker::Pick = Rc::new(move |ix, _, window, cx| {
                let Some((id, run)) = shown.borrow().get(ix).cloned() else { return };
                {
                    let mut r = r2.borrow_mut();
                    r.retain(|x| *x != id);
                    r.insert(0, id.clone());
                    r.truncate(20);
                }
                if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
                    println!("COMMAND {id}");
                }
                let editor = WorkspaceHandle::current(cx).and_then(|w| w.active_editor(cx));
                super::run_action(&run, editor, window, cx);
            });
            picker::open("Type a command…", items, pick, a.window, a.cx);
        });
        reg.command(Command { id: "open".into(), name: "Open command palette".into(), hotkey: Some("ctrl-p".into()), run: open.clone() });
        reg.ribbon(RibbonItem { id: "open".into(), icon: gpui_kit::assets::IconName::SquareTerminal, tooltip: "Command palette (Ctrl+P)".into(), run: open });
    }
}

#[cfg(test)]
mod tests {
    use super::rank;

    #[test]
    fn recent_first_then_quality() {
        let names: Vec<(String, String)> = [("a:x", "Search: Search in all files"), ("b:y", "Create new note"), ("c:z", "Toggle reading view")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let ids = |v: Vec<(usize, Vec<std::ops::Range<usize>>)>| v.into_iter().map(|(i, _)| i).collect::<Vec<_>>();
        assert_eq!(ids(rank(&names, &[], "")), vec![1, 0, 2], "alphabetical");
        assert_eq!(ids(rank(&names, &["c:z".into()], "")), vec![2, 1, 0], "recent first");
        assert_eq!(ids(rank(&names, &[], "note")), vec![1]);
        assert_eq!(ids(rank(&names, &[], "rea")), vec![2, 1, 0], "word start (reading), inside (create), letters in order (search)");
    }
}
