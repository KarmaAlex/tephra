//! Settings → Editing toolbar → layout (M8): your toolbar as a list you rearrange by dragging,
//! next to every command the toolbar can hold. Changes apply to the bar at once and are saved
//! in `.obsidilight/plugins/editing-toolbar.json`; the Editing Toolbar's own settings in
//! `.obsidian/` are only read (they are what "Reset" goes back to).
//!
//! Dropping something on a row puts it just before that row; the dashed strips at the end of
//! the toolbar and of each menu add it at the end. Menus can't go inside menus.

use std::any::Any;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use super::layout::{self, Item, Path, Slot};
use super::{clean_name, describe, icon_for, ops_map, Ops, ID};
use crate::plugins::{PluginHost, WorkspaceHandle};

/// What is being dragged.
#[derive(Clone)]
enum Dragged {
    /// A toolbar entry (`true`: a menu).
    Item(Path, SharedString, bool),
    /// A command from the list on the right (id, name).
    Command(String, SharedString),
}

/// What follows the mouse while dragging (drawn from where the row was grabbed, so it sits
/// by the pointer).
struct Chip(SharedString, Point<Pixels>);

impl Render for Chip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let chip = div().px_2().py_1().rounded(px(4.)).bg(t.popover).border_1().border_color(t.border).shadow_md().text_sm().child(self.0.clone());
        div().pl(self.1.x + px(14.)).pt(self.1.y + px(6.)).child(chip)
    }
}

fn chip(d: &Dragged, offset: Point<Pixels>, cx: &mut App) -> Entity<Chip> {
    let t = match d {
        Dragged::Item(_, t, _) | Dragged::Command(_, t) => t.clone(),
    };
    cx.new(|_| Chip(t, offset))
}

fn current(cx: &App) -> Vec<Item> {
    cx.try_global::<PluginHost>().map(|h| layout::parse(&h.setting(ID, "menuCommands"))).unwrap_or_default()
}

/// Applies `f` to the layout and saves it when it changed something (the bar updates).
fn edit(cx: &mut App, f: impl FnOnce(&mut Vec<Item>) -> bool) {
    let mut items = current(cx);
    if f(&mut items) {
        PluginHost::set_setting(cx, ID, "menuCommands", layout::to_json(&items));
    }
}

fn dropped(d: &Dragged, at: Slot, cx: &mut App) {
    match d {
        Dragged::Item(from, _, _) => edit(cx, |l| layout::move_item(l, *from, at)),
        Dragged::Command(id, name) => edit(cx, |l| layout::insert(l, at, Item::command(id, name))),
    }
}

/// Menus only go to the top level.
fn accepts(d: &dyn Any, at: Slot) -> bool {
    at.parent.is_none() || !matches!(d.downcast_ref::<Dragged>(), Some(Dragged::Item(_, _, true)))
}

fn key(p: Path) -> String {
    match p.child {
        None => format!("{}", p.top),
        Some(j) => format!("{}.{j}", p.top),
    }
}

fn rename_dialog(p: Path, name: String, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
    let focus = input.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, _| {
        let input = input.clone();
        dialog
            .title("Rename")
            .child(Input::new(&input))
            .footer(
                DialogFooter::new()
                    .child(DialogClose::new().child(Button::new("cancel").label("Cancel").outline()))
                    .child(DialogAction::new().child(Button::new("ok").label("Rename").primary())),
            )
            .on_ok(move |_, _, cx| {
                let v = input.read(cx).value().trim().to_string();
                edit(cx, |l| match layout::get_mut(l, p) {
                    Some(it) if it.name != v => {
                        it.name = v;
                        true
                    }
                    _ => false,
                });
                true
            })
    });
    window.focus(&focus, cx);
}

/// The row's text.
fn label(it: &Item, ops: &Ops) -> String {
    if it.is_divider() {
        return if it.name.trim().is_empty() { "Divider".into() } else { format!("Divider: {}", clean_name(&it.name)) };
    }
    let (name, _) = describe(it, ops);
    if it.is_submenu() {
        format!("{name} (menu)")
    } else if name.is_empty() {
        it.command_id().to_string()
    } else {
        name
    }
}

/// A drop target with the accent line at its top while something is dragged over it.
fn target<E: InteractiveElement + Styled + 'static>(el: E, at: Slot) -> E {
    el.drag_over::<Dragged>(|s, _, _, cx| s.border_color(cx.theme().primary))
        .can_drop(move |d, _, _| accepts(d, at))
        .on_drop(move |d: &Dragged, _, cx| dropped(d, at, cx))
}

fn row(p: Path, it: &Item, ops: &Ops, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let text = label(it, ops);
    let available = it.is_divider() || it.is_submenu() || ops.contains_key(it.command_id());
    let icon = if it.is_divider() { IconName::SeparatorVertical } else { describe(it, ops).1 };
    let k = key(p);
    let drag = Dragged::Item(p, text.clone().into(), it.is_submenu());
    let renamable = it.is_divider() || it.is_submenu();
    let name = it.name.clone();
    let row = h_flex()
        .id(SharedString::from(format!("tb-row-{k}")))
        .relative()
        .w_full()
        .h(crate::ui::r(28.))
        .pl(if p.child.is_some() { crate::ui::r(28.) } else { crate::ui::r(4.) })
        .pr_1()
        .gap_2()
        .border_t_2()
        .border_color(transparent_black())
        .rounded(px(4.))
        .hover(|s| s.bg(cx.theme().secondary))
        .cursor_grab()
        .child(Icon::new(IconName::GripVertical).xsmall().text_color(theme.muted_foreground))
        .child(Icon::new(icon).small())
        .child(div().flex_1().min_w_0().truncate().text_sm().when(!available, |d| d.text_color(theme.muted_foreground)).child(text.clone()))
        .when(!available, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child("not in this app")))
        .when(renamable, |d| {
            d.child(crate::ui::icon_button(SharedString::from(format!("tb-rename-{k}")), IconName::Pencil).tooltip("Rename").on_click(move |_, window, cx| rename_dialog(p, name.clone(), window, cx)))
        })
        .child(
            div()
                .relative()
                .child(crate::ui::icon_button(SharedString::from(format!("tb-remove-{k}")), IconName::X).tooltip("Remove from the toolbar").on_click(move |_, _, cx| edit(cx, |l| layout::remove(l, p).is_some())))
                .children(crate::script::probe(|| format!("tb-remove-probe:{k}"), cx)),
        )
        .children(crate::script::probe(|| format!("tb-row:{k}"), cx))
        .on_drag(drag, |d, offset, _, cx| chip(d, offset, cx));
    target(row, Slot::before(p)).into_any_element()
}

/// The strip that adds at the end of the toolbar (`parent` None) or of a menu.
fn end_strip(parent: Option<usize>, len: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let at = Slot { parent, index: len };
    let k = parent.map(|p| p.to_string()).unwrap_or("top".into());
    let text = if parent.is_some() { "Drop here to add at the end of this menu" } else { "Drop here to add at the end of the toolbar" };
    let strip = div()
        .id(SharedString::from(format!("tb-end-{k}")))
        .relative()
        .ml(crate::ui::r(if parent.is_some() { 28. } else { 0. }))
        .h(crate::ui::r(22.))
        .px_2()
        .flex()
        .items_center()
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .rounded(px(4.))
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text)
        .children(crate::script::probe(|| format!("tb-end:{k}"), cx));
    target(strip, at).into_any_element()
}

/// A command in the list on the right: drag it into the toolbar, or + adds it at the end.
fn command_row(id: &'static str, name: &'static str, used: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .id(SharedString::from(format!("tb-cmd-{id}")))
        .relative()
        .w_full()
        .h(crate::ui::r(26.))
        .px_1()
        .gap_2()
        .rounded(px(4.))
        .hover(|s| s.bg(cx.theme().secondary))
        .cursor_grab()
        .child(Icon::new(icon_for(id)).small())
        .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
        .when(used, |d| d.child(Icon::new(IconName::Check).xsmall().text_color(theme.muted_foreground)))
        .child(
            div()
                .relative()
                .child(crate::ui::icon_button(SharedString::from(format!("tb-add-{id}")), IconName::Plus).tooltip("Add at the end of the toolbar").on_click(move |_, _, cx| {
                    edit(cx, |l| {
                        let n = l.len();
                        layout::insert(l, Slot { parent: None, index: n }, Item::command(id, name))
                    })
                }))
                .children(crate::script::probe(|| format!("tb-add-probe:{id}"), cx)),
        )
        .children(crate::script::probe(|| format!("tb-cmd:{id}"), cx))
        .on_drag(Dragged::Command(id.to_string(), name.into()), |d, offset, _, cx| chip(d, offset, cx))
        .into_any_element()
}

/// The layout section of the plugin's settings page.
pub fn render(window: &mut Window, cx: &mut App) -> AnyElement {
    let items = current(cx);
    let ops = Rc::new(ops_map());
    let theme = cx.theme().clone();
    let used = layout::used_commands(&items);

    // left: the toolbar as it is
    let mut left: Vec<AnyElement> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        left.push(row(Path::top(i), it, &ops, cx));
        if let Some(kids) = &it.children {
            for (j, c) in kids.iter().enumerate() {
                left.push(row(Path::child(i, j), c, &ops, cx));
            }
            left.push(end_strip(Some(i), kids.len(), cx));
        }
    }
    left.push(end_strip(None, items.len(), cx));

    // right: every command, by kind
    let mut all: Vec<(&'static str, &'static str)> = super::table().into_iter().map(|(id, name, _, _)| (id, name)).collect();
    all.sort_by_key(|(id, _)| super::CATEGORIES.iter().position(|c| *c == super::category(id)).unwrap_or(usize::MAX));
    let mut right: Vec<AnyElement> = Vec::new();
    let mut last = "";
    for (id, name) in all {
        let cat = super::category(id);
        if cat != last {
            right.push(div().pt_2().pb_0p5().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(cat.to_uppercase()).into_any_element());
            last = cat;
        }
        right.push(command_row(id, name, used.iter().any(|u| u == id), cx));
    }

    // the bar as it will look
    let shown = layout::shown(&items, &|id| ops.contains_key(id));
    let preview = match WorkspaceHandle::current(cx) {
        Some(ws) => super::bar(&shown, &ops, &ws, false, window, cx).into_any_element(),
        None => div().into_any_element(),
    };

    let column = |title: &'static str, body: Vec<AnyElement>| {
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).pb_1().child(title))
            .children(body)
    };
    let buttons = h_flex()
        .gap_2()
        .child(Button::new("tb-new-menu").label("Add menu").icon(IconName::ListPlus).small().outline().on_click(|_, _, cx| {
            edit(cx, |l| {
                let id = layout::new_submenu_id(l);
                l.push(Item::submenu(id, "New menu"));
                true
            })
        }))
        .child(Button::new("tb-new-divider").label("Add divider").icon(IconName::SeparatorVertical).small().outline().on_click(|_, _, cx| {
            edit(cx, |l| {
                l.push(Item::divider(""));
                true
            })
        }))
        .child(div().flex_1())
        .child(Button::new("tb-reset").label("Reset to your Editing Toolbar layout").icon(IconName::RotateCcw).small().ghost().on_click(|_, _, cx| {
            let d = cx.global::<PluginHost>().setting_default(ID, "menuCommands");
            if d != cx.global::<PluginHost>().setting(ID, "menuCommands") {
                PluginHost::set_setting(cx, ID, "menuCommands", if d.is_null() { Value::Array(Vec::new()) } else { d });
            }
        }));

    v_flex()
        .w_full()
        .gap_3()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Toolbar layout"))
        .child(div().text_xs().text_color(theme.muted_foreground).child(
            "Drag the rows to reorder them, or into and out of menus; drag commands in from the right. Saved in .obsidilight; your Editing Toolbar settings in .obsidian stay as they are.",
        ))
        .child(v_flex().gap_1().child(div().text_xs().text_color(theme.muted_foreground).child("Preview")).child(div().p_1().rounded(px(4.)).border_1().border_color(theme.border).child(preview)))
        .child(buttons)
        .child(h_flex().w_full().items_start().gap_4().child(column("In the toolbar", left)).child(column("Commands", right)))
        .into_any_element()
}
