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
        dialog.title("Settings").w(crate::ui::p(cx, 880.)).child(div().h(crate::ui::r(580.)).child(Settings::new("obsidilight-settings").pages(pages(cx).into_iter().map(|(_, p)| p))))
    });
}

/// Opens Settings at the page titled `title` (a plugin's name), or the first page.
pub fn open_at(title: &str, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::setting::SelectIndex;
    let page_ix = pages(cx).iter().position(|(t, _)| t == title).unwrap_or(0);
    // a fresh id: the settings' state (and so the page shown) starts anew
    static OPENED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    window.open_dialog(cx, move |dialog, _, cx| {
        let settings = Settings::new(SharedString::from(format!("obsidilight-settings-{n}"))).pages(pages(cx).into_iter().map(|(_, p)| p)).default_selected_index(SelectIndex { page_ix, group_ix: None });
        dialog.title("Settings").w(crate::ui::p(cx, 880.)).child(div().h(crate::ui::r(580.)).child(settings))
    });
}

/// The pages with their titles.
fn pages(cx: &App) -> Vec<(String, SettingPage)> {
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
    let editor: Vec<SettingItem> = crate::app_settings::TOGGLES
        .iter()
        .map(|(key, label, desc, _)| {
            let key: &'static str = key;
            SettingItem::new(*label, SettingField::switch(move |cx: &App| crate::app_settings::AppSettings::flag(cx, key), move |v: bool, cx: &mut App| crate::app_settings::AppSettings::set(cx, key, Value::Bool(v))))
                .description(*desc)
                .keywords([key])
        })
        .collect();
    let mut pages = vec![
        ("Editor".to_string(), SettingPage::new("Editor").group(SettingGroup::new().items(editor))),
        ("Appearance".to_string(), appearance_page(cx)),
        ("Plugins".to_string(), SettingPage::new("Plugins").description("Bundled plugins. Turning one off removes everything it added; no restart needed.").group(SettingGroup::new().items(toggles))),
        ("Hotkeys".to_string(), crate::hotkeys::settings_page(cx)),
    ];
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
            pages.push((m.name.to_string(), SettingPage::new(m.name).description(m.description).group(SettingGroup::new().items(items))));
        }
    }
    pages
}

/// Settings → Appearance: light/dark, the Obsidian theme, CSS snippets, and what each file
/// contributed (GPUI has no CSS engine: only variables apply; see `theme.rs`).
fn appearance_page(cx: &App) -> SettingPage {
    use gpui_kit::component::button::Button;
    use gpui_kit::component::{v_flex, ActiveTheme as _, Sizable as _};
    let Some(t) = cx.try_global::<crate::theme::ThemeCss>() else { return SettingPage::new("Appearance") };
    let set_and_reload = |key: &'static str, v: Value, cx: &mut App| {
        crate::app_settings::AppSettings::set(cx, key, v);
        crate::theme::reload(cx);
    };
    let mut themes: Vec<(SharedString, SharedString)> = vec![("".into(), "Default".into())];
    themes.extend(t.themes.iter().map(|n| (SharedString::from(n.clone()), SharedString::from(n.clone()))));
    let mut items = vec![
        SettingItem::new(
            "Base color scheme",
            SettingField::dropdown(
                vec![("system".into(), "Adapt to system".into()), ("light".into(), "Light".into()), ("dark".into(), "Dark".into())],
                |cx: &App| SharedString::from(crate::theme::base_theme(cx)),
                move |v: SharedString, cx: &mut App| set_and_reload("baseTheme", Value::from(v.to_string()), cx),
            ),
        )
        .description("Light or dark colours, or follow the system."),
        SettingItem::new(
            "Interface scale",
            SettingField::dropdown(
                [75, 80, 90, 100, 110, 125, 150, 175, 200].iter().map(|p| (SharedString::from(format!("{}", *p as f32 / 100.0)), SharedString::from(format!("{p}%")))).collect(),
                |cx: &App| SharedString::from(format!("{}", (crate::theme::ui_scale(cx) * 100.0).round() / 100.0)),
                |v: SharedString, cx: &mut App| {
                    if let Ok(f) = v.parse::<f64>() {
                        crate::app_settings::AppSettings::set(cx, "uiScale", Value::from(f));
                        crate::theme::apply(None, cx);
                    }
                },
            ),
        )
        .description("Size of everything: text, icons, panels, images. 100% matches Obsidian's default sizes."),
        SettingItem::new(
            "Theme",
            SettingField::dropdown(
                themes,
                |cx: &App| {
                    let root = cx.try_global::<crate::theme::ThemeCss>().map(|t| t.root.clone()).unwrap_or_default();
                    SharedString::from(crate::theme::css_theme(cx, &root))
                },
                move |v: SharedString, cx: &mut App| set_and_reload("cssTheme", Value::from(v.to_string()), cx),
            ),
        )
        .description("An Obsidian theme from .obsidian/themes. Its CSS variables apply (colours, fonts, sizes); other rules can't."),
    ];
    for r in t.reports.iter().filter(|r| !r.is_theme) {
        let name = r.name.clone();
        let mut desc = Vec::new();
        if r.global_vars > 0 {
            desc.push(format!("{} global variable(s){}", r.global_vars, if r.enabled { "" } else { ", applied when on" }));
        }
        if !r.classes.is_empty() {
            desc.push(format!("{} variable(s) for notes with cssclasses: {} (always on)", r.class_vars, r.classes.join(", ")));
        }
        if r.callouts > 0 {
            desc.push(format!("{} callout colour(s)", r.callouts));
        }
        if r.ignored > 0 {
            desc.push(format!("{} rule(s) ignored", r.ignored));
        }
        let n1 = name.clone();
        items.push(
            SettingItem::new(
                SharedString::from(format!("Snippet: {name}")),
                SettingField::switch(
                    move |cx: &App| cx.try_global::<crate::theme::ThemeCss>().is_some_and(|t| t.reports.iter().any(|r| r.name == n1 && r.enabled && !r.is_theme)),
                    move |on: bool, cx: &mut App| {
                        let root = cx.try_global::<crate::theme::ThemeCss>().map(|t| t.root.clone()).unwrap_or_default();
                        let mut list = crate::theme::enabled_snippets(cx, &root);
                        list.retain(|n| *n != name);
                        if on {
                            list.push(name.clone());
                        }
                        set_and_reload("enabledCssSnippets", Value::from(list), cx);
                    },
                ),
            )
            .description(SharedString::from(desc.join(" · "))),
        );
    }
    // what each file contributed, with examples of ignored rules
    let reports = t.reports.clone();
    items.push(SettingItem::render(move |_, _, cx| {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .w_full()
            .child(div().text_xs().text_color(muted).child("Only CSS variables apply here: rules that style elements directly (h1 { … }, table td { … }), @media and @font-face are ignored. Snippets are read from .obsidian/snippets; class-scoped variables (e.g. .NoBorder) apply to notes listing the class in cssclasses, even when the snippet is off."))
            .children(reports.iter().filter(|r| r.ignored > 0).map(|r| {
                v_flex()
                    .gap_0p5()
                    .child(div().text_sm().child(format!("{} {}: {} rule(s) ignored, e.g.", if r.is_theme { "Theme" } else { "Snippet" }, r.name, r.ignored)))
                    .children(r.examples.iter().map(|e| div().pl_3().text_xs().text_color(muted).font_family(cx.theme().mono_font_family.clone()).child(e.clone())))
            }))
            .child(Button::new("reload-css").label("Reload themes and snippets").small().outline().on_click(|_, _, cx| crate::theme::reload(cx)))
            .into_any_element()
    }));
    SettingPage::new("Appearance").description("Light and dark colours, Obsidian themes and CSS snippets.").group(SettingGroup::new().items(items))
}
