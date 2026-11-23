//! A modal list with a query field (quick switcher, command palette): the plugin computes the
//! items for each query; ↑/↓ choose, Enter picks, Ctrl+Enter picks "in a new tab", Esc closes.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::{h_flex, ActiveTheme as _, WindowExt as _};
use gpui_kit::*;

use super::search::highlighted;

#[derive(Clone, Debug, Default)]
pub struct PickItem {
    pub label: String,
    /// Byte ranges of `label` that matched the query.
    pub hl: Vec<Range<usize>>,
    /// Muted text after the label (a folder).
    pub detail: String,
    /// Right-aligned hint (a hotkey).
    pub hint: String,
}

/// `items(query, cx)` runs on every render of the dialog; `pick(index, secondary, …)` runs
/// after the dialog closed (`secondary` = Ctrl+Enter).
pub type Items = Rc<dyn Fn(&str, &App) -> Vec<PickItem>>;
pub type Pick = Rc<dyn Fn(usize, bool, &mut Window, &mut App)>;

/// Prints what a picker shows (`PICKER …` lines) under `OBSIDILIGHT_SCRIPT`, for UI tests.
fn trace(query: &str, items: &[PickItem]) {
    if std::env::var_os("OBSIDILIGHT_SCRIPT").is_some() {
        let first: Vec<&str> = items.iter().take(5).map(|i| i.label.as_str()).collect();
        println!("PICKER {query:?} {} {first:?}", items.len());
    }
}

pub fn open(placeholder: &'static str, items: Items, pick: Pick, window: &mut Window, cx: &mut App) {
    let state = cx.new(|cx| CommandState::new(window, cx));
    let shown: Rc<RefCell<(String, Vec<PickItem>)>> = Rc::default();
    let st = state.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let q = st.read(cx).query(cx).to_string();
        let list = items(&q, cx);
        {
            let mut s = shown.borrow_mut();
            if s.0 != q || s.1.len() != list.len() {
                trace(&q, &list);
            }
            *s = (q, list.clone());
        }
        let theme = cx.theme().clone();
        let (muted, mark) = (theme.muted_foreground, theme.warning.opacity(0.3));
        let rows = list.into_iter().map(|it| {
            let it2 = it.clone();
            CommandItem::new().label(it.label.clone()).child(move |_, _| {
                h_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .child(div().flex_none().max_w(px(380.)).truncate().child(highlighted(it2.label.clone(), &it2.hl, mark)))
                    .child(div().flex_1().min_w_0().truncate().text_xs().text_color(muted).child(it2.detail.clone()))
                    .child(div().flex_none().text_xs().text_color(muted).child(it2.hint.clone()))
            })
        });
        let (p1, p2, st2) = (pick.clone(), pick.clone(), st.clone());
        dialog.w(px(640.)).margin_top(px(80.)).close_button(false).child(
            div()
                // Esc closes at once (the list's own Cancel would first clear the query)
                .capture_action(|_: &gpui_kit::base::actions::Cancel, window, cx| {
                    cx.stop_propagation();
                    window.close_dialog(cx);
                })
                .capture_key_down(move |ev: &KeyDownEvent, window, cx| {
                    let k = &ev.keystroke;
                    if k.key == "enter" && k.modifiers.control {
                        cx.stop_propagation();
                        if let Some(ix) = st2.read(cx).selected_index() {
                            window.close_dialog(cx);
                            p2(ix.row, true, window, cx);
                        }
                    }
                })
                .child(
                    Command::new(&st)
                        .filterable(false)
                        .bordered(false)
                        .placeholder(placeholder)
                        .max_h(px(420.))
                        .items(rows)
                        .on_confirm(move |ix, window, cx| {
                            window.close_dialog(cx);
                            p1(ix.row, false, window, cx);
                        })
                        .on_cancel(|window, cx| window.close_dialog(cx)),
                ),
        )
    });
    state.update(cx, |s, cx| s.focus(window, cx));
}

/// `ctrl-shift-f` → `Ctrl+Shift+F`.
pub fn hotkey_label(h: &str) -> String {
    h.split('-')
        .map(|p| match p {
            "ctrl" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" => "Alt".to_string(),
            "cmd" | "super" => "Super".to_string(),
            k if k.chars().count() == 1 => k.to_uppercase(),
            k => {
                let mut c = k.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}
