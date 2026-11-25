//! Setting rows: the title and description on the left, the control in a fixed-width column on the
//! right. gpui-kit's setting items size each control by its type (a 256 px text box, a 128 px
//! number box, a switch or a dropdown as wide as its label), so no two rows line up, and a long
//! word in a description can push a text box past the edge of the page. Every page of the Settings
//! window builds its rows here instead, so all submenus share one layout.

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::setting::SettingItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Width of the column the controls sit in (100% UI scale), right-aligned in every row.
pub const FIELD: f32 = 200.;

/// The row a control is drawn in: its element id (unique per page, group and item), its title,
/// which the control uses as its accessible name, and its script key (see [`slug`]).
pub struct Row {
    pub id: SharedString,
    pub title: SharedString,
    pub key: SharedString,
}

/// The `key` in `settings:<key>:field` and `settings:<key>:control`, the probes scripts click and
/// measure (see [`crate::script::probe`]): the title in lower case, with runs of anything else as
/// one dash.
pub fn slug(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// A setting row: `title` and `description` on the left, `control` in the field column. The row is
/// found by its title, its description and any `keywords` (a settings key, such as `uiScale`).
pub fn item<C>(title: impl Into<SharedString>, description: impl Into<SharedString>, keywords: Vec<SharedString>, control: C) -> SettingItem
where
    C: Fn(&Row, &mut Window, &mut App) -> AnyElement + 'static,
{
    let title: SharedString = title.into();
    let description: SharedString = description.into();
    let mut searchable = vec![title.clone(), description.clone()];
    searchable.extend(keywords);
    SettingItem::render(move |options, window, cx| {
        let key = slug(&title);
        let row = Row {
            id: SharedString::from(format!("setting-{}-{}-{}", options.page_ix(), options.group_ix(), options.item_ix())),
            title: title.clone(),
            key: SharedString::from(key.clone()),
        };
        let muted = cx.theme().muted_foreground;
        let description = description.clone();
        h_flex()
            .w_full()
            .items_start()
            .gap(crate::ui::r(16.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().child(row.title.clone()))
                    .when(!description.is_empty(), |this| this.child(div().text_sm().text_color(muted).child(description))),
            )
            .child(
                div()
                    .relative()
                    .w(crate::ui::r(FIELD))
                    .flex_none()
                    .child(h_flex().w_full().justify_end().child(control(&row, window, cx)))
                    .children(crate::script::probe(move || format!("settings:{key}:field"), cx)),
            )
            .into_any_element()
    })
    .keywords(searchable)
}

/// A switch, right-aligned in the field column.
pub fn switch<G, S>(get: G, set: S) -> impl Fn(&Row, &mut Window, &mut App) -> AnyElement
where
    G: Fn(&App) -> bool + 'static,
    S: Fn(bool, &mut App) + 'static,
{
    let set = Rc::new(set);
    move |row, _, cx| {
        let set = set.clone();
        let label = row.title.clone();
        let key = row.key.clone();
        div()
            .relative()
            .flex_none()
            .child(
                Switch::new(row.id.clone())
                    .accessibility_label(label)
                    .checked(get(cx))
                    .on_click(move |on: &bool, _, cx| set(*on, cx)),
            )
            .children(crate::script::probe(move || format!("settings:{key}:control"), cx))
            .into_any_element()
    }
}

/// A dropdown: a button showing the current option, as wide as the field column.
pub fn dropdown<G, S>(options: Vec<(SharedString, SharedString)>, get: G, set: S) -> impl Fn(&Row, &mut Window, &mut App) -> AnyElement
where
    G: Fn(&App) -> SharedString + 'static,
    S: Fn(SharedString, &mut App) + 'static,
{
    let options = Rc::new(options);
    let set = Rc::new(set);
    move |row, _, cx| {
        let current = get(cx);
        let shown = options.iter().find(|(v, _)| *v == current).map(|(_, l)| l.clone()).unwrap_or_else(|| current.clone());
        let (options, set) = (options.clone(), set.clone());
        Button::new(row.id.clone())
            .outline()
            .w(crate::ui::r(FIELD))
            .label(shown.clone())
            .accessibility_label(SharedString::from(format!("{}: {}", row.title, shown)))
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for (value, label) in options.iter() {
                    let set = set.clone();
                    let value = value.clone();
                    menu = menu.item(PopupMenuItem::new(label.clone()).checked(value == current).on_click(move |_, _, cx| set(value.clone(), cx)));
                }
                menu
            })
            .into_any_element()
    }
}

/// A one-line text box, as wide as the field column.
pub fn input<G, S>(get: G, set: S) -> impl Fn(&Row, &mut Window, &mut App) -> AnyElement
where
    G: Fn(&App) -> SharedString + 'static,
    S: Fn(SharedString, &mut App) + 'static,
{
    let set = Rc::new(set);
    move |row, window, cx| {
        let value = get(cx);
        let on_change = set.clone();
        let state = window.use_keyed_state(row.id.clone(), cx, {
            let value = value.clone();
            move |window, cx| {
                let text = cx.new(|cx| InputState::new(window, cx).default_value(value));
                let sub = cx.subscribe(&text, move |_: &mut TextBox, text, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        on_change(text.read(cx).value(), cx);
                    }
                });
                TextBox { text, _sub: sub }
            }
        });
        // Show a value changed elsewhere: another page, or a reset.
        state.update(cx, |field, cx| {
            if field.text.read(cx).value() != value {
                field.text.update(cx, |text, cx| text.set_value(value.clone(), window, cx));
            }
        });
        Input::new(&state.read(cx).text).aria_label(row.title.clone()).w_full().into_any_element()
    }
}

/// A number box, as wide as the field column.
pub fn number<G, S>(get: G, set: S) -> impl Fn(&Row, &mut Window, &mut App) -> AnyElement
where
    G: Fn(&App) -> f64 + 'static,
    S: Fn(f64, &mut App) + 'static,
{
    let set = Rc::new(set);
    move |row, window, cx| {
        let value = get(cx);
        let on_change = set.clone();
        let state = window.use_keyed_state(row.id.clone(), cx, move |window, cx| {
            let text = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()).step(1.0));
            let sub = cx.subscribe(&text, move |field: &mut NumberBox, text, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    // Unparsable intermediates ("-", "", "1.") are left alone so the next
                    // keystroke can finish them.
                    if let Ok(parsed) = text.read(cx).value().parse::<f64>() {
                        if parsed != field.value {
                            field.value = parsed;
                            on_change(parsed, cx);
                        }
                    }
                }
            });
            NumberBox { text, value, _sub: sub }
        });
        // Show a value changed elsewhere, and leave the text alone while it is being typed.
        state.update(cx, |field, cx| {
            if field.value != value {
                field.value = value;
                field.text.update(cx, |text, cx| text.set_value(SharedString::from(value.to_string()), window, cx));
            }
        });
        NumberInput::new(&state.read(cx).text).accessibility_label(row.title.clone()).w_full().into_any_element()
    }
}

struct TextBox {
    text: Entity<InputState>,
    _sub: Subscription,
}

struct NumberBox {
    text: Entity<InputState>,
    value: f64,
    _sub: Subscription,
}

