//! Drawing projected nodes that gpui-kit does not draw itself: callouts (core Obsidian look, or
//! a plugin's style per D7), plugin-rendered code blocks, and noted blocks.

use std::collections::HashMap;

use gpui_kit::base::text::editor_host::{BlockNode, FlowPosition, RenderContext};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, ActiveTheme as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use obsidilight_core::Selection;

use super::projection::{capitalize, CalloutView, ProjNode};
use super::Editor;
use crate::plugins::{CalloutLook, CalloutStyle, PluginHost};

fn rgb_of(r: u8, g: u8, b: u8) -> Hsla {
    rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
}

/// Obsidian's built-in callout types (aliases included): colour (light, dark) and icon.
/// Unknown types look like `note`.
pub fn obsidian_style(kind: &str, dark: bool) -> CalloutStyle {
    // Obsidian's --color-*-rgb in the light and dark base themes
    let (blue, cyan, green, orange, red, purple, grey) = if dark {
        ((2, 122, 255), (83, 223, 221), (68, 207, 110), (233, 151, 63), (251, 70, 76), (168, 130, 255), (158, 158, 158))
    } else {
        ((8, 109, 221), (0, 191, 188), (8, 185, 78), (236, 117, 0), (233, 49, 71), (120, 82, 238), (158, 158, 158))
    };
    let (c, icon) = match kind {
        "abstract" | "summary" | "tldr" => (cyan, IconName::ClipboardList),
        "info" => (blue, IconName::Info),
        "todo" => (blue, IconName::CircleCheck),
        "tip" | "hint" | "important" => (cyan, IconName::Flame),
        "success" | "check" | "done" => (green, IconName::Check),
        "question" | "help" | "faq" => (orange, IconName::CircleQuestionMark),
        "warning" | "caution" | "attention" => (orange, IconName::TriangleAlert),
        "failure" | "fail" | "missing" => (red, IconName::X),
        "danger" | "error" => (red, IconName::Zap),
        "bug" => (red, IconName::Bug),
        "example" => (purple, IconName::List),
        "quote" | "cite" => (grey, IconName::Quote),
        _ => (blue, IconName::Pencil), // note and unknown types
    };
    CalloutStyle { color: rgb_of(c.0, c.1, c.2), look: CalloutLook::Obsidian, icon: Some(icon), label: None }
}

/// The style in effect: the first plugin claiming the type, else Obsidian's.
pub fn callout_style(kind: &str, meta: &str, cx: &App) -> CalloutStyle {
    let dark = cx.theme().is_dark();
    cx.try_global::<PluginHost>().and_then(|h| h.reg.callout_style(kind, meta, dark)).unwrap_or_else(|| obsidian_style(kind, dark))
}

/// Renders a projected node; gpui-kit renders plain blocks.
pub fn render_node(node: &ProjNode, ix: usize, folds: &HashMap<String, bool>, rcx: &RenderContext, window: &mut Window, cx: &mut Context<Editor>) -> AnyElement {
    match node {
        ProjNode::Block(b) => b.render(ix, FlowPosition::at(&[b], 0, true).first(true).last(true), rcx, window, cx),
        ProjNode::Callout(c) => render_callout(c, ix, folds, rcx, window, cx),
        ProjNode::Noted { note, child } => {
            let muted = cx.theme().muted_foreground;
            let el = child.render(ix, FlowPosition::at(&[child], 0, true).first(true).last(true), rcx, window, cx);
            div().flex().flex_col().child(div().text_xs().text_color(muted).child(note.clone())).child(el).into_any_element()
        }
        ProjNode::Rendered { lang, code, at } => {
            let renderer = cx.try_global::<PluginHost>().and_then(|h| h.reg.code_block_renderer(lang));
            let inner = match renderer {
                Some(r) => r.render(code, window, cx),
                None => div().child(code.clone()).into_any_element(),
            };
            let at = *at;
            // clicking a rendered block puts the caret in it, which reveals its source
            div()
                .id(SharedString::from(format!("rendered-{ix}-{at}")))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |e, _: &MouseDownEvent, window, cx| {
                        window.focus(&e.focus, cx);
                        e.set_selection(Selection::caret(at), false, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(inner)
                .into_any_element()
        }
    }
}

fn render_callout(c: &CalloutView, ix: usize, folds: &HashMap<String, bool>, rcx: &RenderContext, window: &mut Window, cx: &mut Context<Editor>) -> AnyElement {
    let style = callout_style(&c.kind, &c.meta, cx);
    let color = style.color;
    let collapsed = c.fold.is_some() && folds.get(&c.fold_key).copied().unwrap_or(c.fold == Some('-'));
    let title = BlockNode::Paragraph(c.title.clone());
    let title_el = title.render(ix, FlowPosition::at(&[&title], 0, true).first(true).last(true), rcx, window, cx);
    let fold_btn = c.fold.map(|_| {
        let key = c.fold_key.clone();
        let default = c.fold == Some('-');
        div()
            .id(SharedString::from(format!("fold-{}", c.fold_key)))
            .cursor_pointer()
            .text_color(color)
            .child(if collapsed { "▸" } else { "▾" })
            .on_click(cx.listener(move |e, _: &ClickEvent, _, cx| {
                let cur = e.folds.get(&key).copied().unwrap_or(default);
                e.folds.insert(key.clone(), !cur);
                cx.notify();
            }))
    });
    let body: Vec<AnyElement> = if collapsed { Vec::new() } else { c.children.iter().map(|ch| render_node(ch, ix, folds, rcx, window, cx)).collect() };
    let header = match style.look {
        CalloutLook::Obsidian => {
            // Obsidian: icon + title in the accent colour; no title -> the capitalised type
            let fallback = c.title_empty.then(|| capitalize(&c.kind));
            h_flex()
                .gap_2()
                .items_center()
                .text_color(color)
                .font_weight(FontWeight::SEMIBOLD)
                .children(style.icon.map(|i| div().flex_none().child(i)))
                .child(div().flex_1().min_w_0().children(fallback).when(!c.title_empty, |d| d.child(title_el)))
                .children(fold_btn)
        }
        CalloutLook::Soft => {
            let label = style.label.clone().unwrap_or_else(|| capitalize(&c.kind));
            h_flex()
                .gap_2()
                .items_center()
                .children(fold_btn)
                .child(div().italic().font_weight(FontWeight::SEMIBOLD).text_color(color).child(format!("{label}.")))
                .child(div().flex_1().min_w_0().italic().child(title_el))
        }
        CalloutLook::Badge | CalloutLook::Rules => {
            let label = style.label.clone().unwrap_or_else(|| capitalize(&c.kind));
            let badge = if style.look == CalloutLook::Badge {
                div().px_2().py_0p5().rounded(px(4.)).bg(color).text_color(gpui_kit::white()).text_sm().font_weight(FontWeight::SEMIBOLD).child(label)
            } else {
                div().font_weight(FontWeight::BOLD).text_color(color).child(label)
            };
            h_flex().gap_2().items_center().children(fold_btn).child(badge).child(div().flex_1().min_w_0().child(title_el))
        }
    };
    let el = div().my_1().flex().flex_col().gap_1().child(header).children(body);
    let el = match style.look {
        CalloutLook::Obsidian => el.pl(px(24.)).pr(px(12.)).py(px(12.)).rounded(px(4.)).bg(color.opacity(0.1)),
        CalloutLook::Badge => el.px_3().pt_2().pb_2().border(px(1.4)).border_color(color).rounded(px(6.)).bg(color.opacity(0.06)),
        CalloutLook::Soft => el.pl_3().border_l_2().border_color(color.opacity(0.6)),
        CalloutLook::Rules => el.px_2().py_1().border_t_2().border_b_2().border_color(color),
    };
    el.into_any_element()
}
