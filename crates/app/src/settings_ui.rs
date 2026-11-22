//! The settings window (Ctrl+, or the ribbon gear): a Plugins page with on/off toggles, then one
//! page per enabled plugin, generated from its manifest's settings schema plus any custom
//! sections it registered. Changes apply immediately and are saved in `.obsidilight/`.

use gpui_kit::component::setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;
use serde_json::Value;

use crate::plugins::{PluginHost, SettingKind};

pub fn open(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, cx| {
        dialog.title("Settings").w(px(880.)).child(div().h(px(580.)).child(Settings::new("obsidilight-settings").pages(pages(cx))))
    });
}

fn pages(cx: &App) -> Vec<SettingPage> {
    let Some(host) = cx.try_global::<PluginHost>() else { return Vec::new() };
    let manifests = host.manifests();
    let toggles: Vec<SettingItem> = manifests
        .iter()
        .map(|m| {
            let id = m.id;
            SettingItem::new(m.name, SettingField::switch(move |cx: &App| cx.global::<PluginHost>().is_enabled(id), move |on: bool, cx: &mut App| PluginHost::set_enabled(cx, id, on)))
                .description(m.description)
                .keywords([m.id])
        })
        .collect();
    let mut pages = vec![SettingPage::new("Plugins").description("Bundled plugins. Turning one off removes everything it added; no restart needed.").group(SettingGroup::new().items(toggles))];
    for m in manifests.iter().filter(|m| host.is_enabled(m.id)) {
        let id = m.id;
        let mut items: Vec<SettingItem> = Vec::new();
        for spec in m.settings.iter().filter(|s| !matches!(s.kind, SettingKind::Hidden)) {
            let key = spec.key;
            let get = move |cx: &App| cx.global::<PluginHost>().setting(id, key);
            let label = spec.label;
            let mut item = match &spec.kind {
                SettingKind::Toggle => SettingItem::new(label, SettingField::switch(move |cx: &App| get(cx).as_bool().unwrap_or(false), move |v: bool, cx: &mut App| PluginHost::set_setting(cx, id, key, Value::Bool(v)))),
                SettingKind::Number => SettingItem::new(label, SettingField::number_input(
                    NumberFieldOptions::default(),
                    move |cx: &App| get(cx).as_f64().unwrap_or(0.0),
                    move |v: f64, cx: &mut App| PluginHost::set_setting(cx, id, key, serde_json::json!(v)),
                )),
                SettingKind::Choice(opts) => SettingItem::new(label, SettingField::dropdown(
                    opts.iter().map(|(v, l)| (SharedString::from(*v), SharedString::from(*l))).collect(),
                    move |cx: &App| SharedString::from(get(cx).as_str().unwrap_or("").to_string()),
                    move |v: SharedString, cx: &mut App| PluginHost::set_setting(cx, id, key, Value::from(v.to_string())),
                )),
                SettingKind::Text => SettingItem::new(label, SettingField::input(
                    move |cx: &App| SharedString::from(get(cx).as_str().unwrap_or("").to_string()),
                    move |v: SharedString, cx: &mut App| {
                        // reload only when the value really changed (typing fires per keystroke)
                        if cx.global::<PluginHost>().setting(id, key).as_str() != Some(v.as_ref()) {
                            PluginHost::set_setting(cx, id, key, Value::from(v.to_string()))
                        }
                    },
                )),
                SettingKind::Hidden => continue,
            };
            item = item.keywords([spec.key]);
            if !spec.description.is_empty() {
                item = item.description(spec.description);
            }
            items.push(item);
        }
        for (_, tab) in host.reg.settings_tabs.iter().filter(|(p, _)| *p == id) {
            let render = tab.render.clone();
            items.push(SettingItem::render(move |_, window, cx| render(window, cx)).keywords([tab.title.clone()]));
        }
        if !items.is_empty() {
            pages.push(SettingPage::new(m.name).description(m.description).group(SettingGroup::new().items(items)));
        }
    }
    pages
}
