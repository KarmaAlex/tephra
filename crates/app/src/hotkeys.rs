//! Hotkeys: every command (the app's and the plugins') runs from its hotkey, through one
//! keystroke interceptor that sees keys before any other binding, so your hotkeys win over the
//! editor's keys. You change them in Settings → Hotkeys; they are saved in
//! `.tephra/hotkeys.json` (`{"command-id": ["ctrl-shift-k"]}`; an empty list = none).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{h_flex, ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::plugins::picker::hotkey_label;
use crate::plugins::{run_action, PluginHost};

/// `ctrl-shift-f` style hotkey vs a key press (modifier order doesn't matter).
pub fn matches(hotkey: &str, k: &Keystroke) -> bool {
    let parts: Vec<&str> = hotkey.split('-').collect();
    let Some((key, mods)) = parts.split_last() else { return false };
    // "ctrl--" style: the key itself is "-"
    let (key, mods) = if key.is_empty() && !mods.is_empty() { (&"-", &mods[..mods.len() - 1]) } else { (key, mods) };
    let has = |m: &str| mods.iter().any(|x| x.eq_ignore_ascii_case(m));
    let m = &k.modifiers;
    key.eq_ignore_ascii_case(&k.key) && m.control == has("ctrl") && m.alt == has("alt") && m.shift == has("shift") && m.platform == (has("cmd") || has("super"))
}

/// A key press as a hotkey string (`ctrl-shift-f`); None for a lone modifier.
pub fn of_keystroke(k: &Keystroke) -> Option<String> {
    if k.is_modifier_key() || k.key.is_empty() {
        return None;
    }
    let m = &k.modifiers;
    let mut parts: Vec<&str> = Vec::new();
    if m.control {
        parts.push("ctrl");
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    if m.platform {
        parts.push("super");
    }
    let key = k.key.to_lowercase();
    Some(if parts.is_empty() { key } else { format!("{}-{key}", parts.join("-")) })
}

/// The command whose hotkey is being recorded in Settings → Hotkeys.
#[derive(Default)]
struct Recording(Option<String>);
impl Global for Recording {}

fn recording(cx: &App) -> Option<String> {
    cx.try_global::<Recording>().and_then(|r| r.0.clone())
}

/// Runs commands from their hotkeys (and records new hotkeys). Installed once at start-up.
pub fn install(cx: &mut App) {
    cx.set_global(Recording::default());
    cx.intercept_keystrokes(|ev, window, cx| {
        if let Some(id) = recording(cx) {
            let k = &ev.keystroke;
            if k.key == "escape" && !k.modifiers.modified() {
                cx.global_mut::<Recording>().0 = None;
            } else if let Some(h) = of_keystroke(k) {
                let mut keys = cx.try_global::<PluginHost>().and_then(|p| p.reg.commands.iter().find(|(_, c)| c.id == id).map(|(_, c)| p.hotkeys_of(c))).unwrap_or_default();
                if !keys.contains(&h) {
                    keys.push(h);
                }
                cx.global_mut::<Recording>().0 = None;
                PluginHost::set_hotkeys(cx, &id, Some(keys));
            } else {
                return; // a modifier on its own: keep waiting
            }
            cx.refresh_windows();
            cx.stop_propagation();
            return;
        }
        // dialogs (settings, pickers) keep their keys
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(cmd) = cx.try_global::<PluginHost>().and_then(|h| h.command_for(&ev.keystroke)) else { return };
        cx.stop_propagation();
        let editor = crate::plugins::WorkspaceHandle::current(cx).and_then(|w| w.active_editor(cx));
        let run = cmd.run.clone();
        window.defer(cx, move |window, cx| run_action(&run, editor, window, cx));
    })
    .detach();
}

/// Settings → Hotkeys: every command with its hotkeys; record, remove, or reset each.
pub fn settings_page(cx: &App) -> SettingPage {
    let Some(host) = cx.try_global::<PluginHost>() else { return SettingPage::new("Hotkeys") };
    let mut cmds: Vec<(String, String)> = host
        .reg
        .commands
        .iter()
        .map(|(p, c)| (c.id.clone(), match host.plugin_name(p) { Some(n) => format!("{n}: {}", c.name), None => c.name.clone() }))
        .collect();
    cmds.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    let items: Vec<SettingItem> = cmds
        .into_iter()
        .map(|(id, name)| {
            let keywords = name.clone();
            SettingItem::render(move |_, _, cx| row(&id, &name, cx)).keywords([keywords])
        })
        .collect();
    SettingPage::new("Hotkeys")
        .description("Click + and press the keys for a command; Esc cancels. Hotkeys work everywhere in the window except inside dialogs, and win over the editor's own keys.")
        .group(SettingGroup::new().items(items))
}

fn row(id: &str, name: &str, cx: &App) -> AnyElement {
    let Some(host) = cx.try_global::<PluginHost>() else { return div().into_any_element() };
    let Some(cmd) = host.reg.commands.iter().map(|(_, c)| c).find(|c| c.id == id) else { return div().into_any_element() };
    let keys = host.hotkeys_of(cmd);
    let overridden = host.hotkey_overridden(id);
    let theme = cx.theme();
    // other commands with the same hotkey
    let clashes: Vec<String> = keys
        .iter()
        .flat_map(|k| host.reg.commands.iter().filter(move |(_, c)| c.id != cmd.id && host.hotkeys_of(c).contains(k)).map(|(_, c)| c.name.clone()))
        .collect();
    let rec = recording(cx).as_deref() == Some(id);
    let id: Rc<str> = id.into();
    let chips = keys.iter().enumerate().map(|(i, k)| {
        let (id, k2) = (id.clone(), k.clone());
        h_flex()
            .gap_1()
            .px_2()
            .rounded(px(4.))
            .bg(theme.secondary)
            .text_xs()
            .child(hotkey_label(k))
            .child(Button::new(SharedString::from(format!("rm-{id}-{i}"))).ghost().xsmall().label("×").on_click(move |_, _, cx| {
                let keys: Vec<String> = cx.global::<PluginHost>().reg.commands.iter().find(|(_, c)| *c.id == *id).map(|(_, c)| cx.global::<PluginHost>().hotkeys_of(c)).unwrap_or_default();
                PluginHost::set_hotkeys(cx, &id, Some(keys.into_iter().filter(|x| *x != k2).collect()));
            }))
    });
    let (id_rec, id_reset) = (id.clone(), id.clone());
    h_flex()
        .w_full()
        .gap_2()
        .py_1()
        .child(div().flex_1().min_w_0().text_sm().child(name.to_string()).when(!clashes.is_empty(), |d| d.child(div().text_xs().text_color(theme.danger).child(format!("Same hotkey as: {}", clashes.join(", "))))))
        .children(chips)
        .child(if rec {
            div().text_xs().text_color(theme.primary).child("Press keys…").into_any_element()
        } else {
            Button::new(SharedString::from(format!("rec-{id}"))).ghost().xsmall().label("+").tooltip("Add a hotkey").on_click(move |_, _, cx| {
                cx.global_mut::<Recording>().0 = Some(id_rec.to_string());
                cx.refresh_windows();
            }).into_any_element()
        })
        .when(overridden, |d| {
            d.child(Button::new(SharedString::from(format!("reset-{id}"))).ghost().xsmall().label("Reset").tooltip("Back to the default").on_click(move |_, _, cx| PluginHost::set_hotkeys(cx, &id_reset, None)))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{matches, of_keystroke};
    use gpui_kit::Keystroke;

    #[test]
    fn hotkey_strings_round_trip() {
        for h in ["ctrl-shift-f", "f2", "ctrl-,", "alt-super-k", "ctrl-tab"] {
            let k = Keystroke::parse(h).unwrap();
            assert!(matches(h, &k), "{h}");
            let back = of_keystroke(&k).unwrap();
            assert!(matches(&back, &k), "{h} -> {back}");
        }
        assert!(matches("shift-ctrl-f", &Keystroke::parse("ctrl-shift-f").unwrap()), "order doesn't matter");
        assert!(!matches("ctrl-f", &Keystroke::parse("ctrl-shift-f").unwrap()));
        assert_eq!(of_keystroke(&Keystroke::parse("shift").unwrap()), None);
    }
}
