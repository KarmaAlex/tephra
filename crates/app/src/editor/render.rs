//! Drawing projected nodes that gpui-kit does not draw itself: callouts (core Obsidian look, or
//! a plugin's style per D7), plugin-rendered code blocks, and noted blocks.

use std::collections::HashMap;

use gpui_kit::base::text::editor_host::{BlockNode, FlowPosition, RenderContext};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, ActiveTheme as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tephra_core::Selection;

use std::path::PathBuf;
use std::rc::Rc;

use super::projection::{capitalize, project, CalloutView, CodeMode, Palette, ProjNode, ProjectionInput, ViewMode};
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
    callout_style_for(kind, meta, cx.theme().is_dark(), cx)
}

/// The style in the light or dark colours, whatever the app shows (PDFs are always light).
pub fn callout_style_for(kind: &str, meta: &str, dark: bool, cx: &App) -> CalloutStyle {
    let mut s = cx.try_global::<PluginHost>().and_then(|h| h.reg.callout_style(kind, meta, dark)).unwrap_or_else(|| obsidian_style(kind, dark));
    // a snippet's `.callout[data-callout="kind"] { --callout-color }` wins, as it would in CSS
    if let Some(c) = cx.try_global::<crate::theme::ThemeCss>().and_then(|t| t.callout_color(kind, dark)) {
        s.color = c;
    }
    s
}

/// Fixes C++ highlighting: gpui-kit registers tree-sitter-cpp with only its own highlight
/// rules, which extend C's (the grammar inherits C's), so types, `return`, strings and
/// comments got no colour. C++ gets C's rules followed by its own, as editors combine them.
pub fn register_languages() {
    use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};
    let reg = LanguageRegistry::singleton();
    let (Some(c), Some(cpp)) = (reg.language("c"), reg.language("cpp")) else { return };
    if cpp.highlights.contains(c.highlights.as_ref()) {
        return; // already combined (a newer gpui-kit)
    }
    let highlights = format!("{}\n{}", c.highlights, cpp.highlights);
    let fixed = GrammarConfig { highlights: highlights.into(), ..cpp.clone() };
    reg.register(cpp.name.as_ref(), &fixed);
}

/// Text colour readable on a filled `bg`: near-black on light colours (the dark palettes'
/// pastel badges), white on dark ones (WCAG relative luminance).
pub fn on_color(bg: Hsla) -> Hsla {
    let c = gpui_kit::Rgba::from(bg);
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let l = 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
    // contrast with white vs with #1a1a1a (luminance ~0.010): pick the larger
    if 1.05 / (l + 0.05) >= (l + 0.05) / 0.0603 { gpui_kit::white() } else { gpui_kit::rgb(0x1a1a1a).into() }
}

/// Renders Markdown read-only (for plugins: e.g. the columns of a layout block), with links and
/// embeds resolved from the note it appears in.
#[derive(Clone)]
pub struct MarkdownRenderer {
    pub resolve: Rc<dyn Fn(&str) -> Option<PathBuf>>,
    depth: usize,
    /// Each render gets its own element-id scope (renders happen in the same order every
    /// frame, so the ids are stable); otherwise identical inline objects would collide.
    next: Rc<std::cell::Cell<usize>>,
    /// Plugin callout numbers for the rendered text, by line (previews of numbered callouts).
    numbers: Option<Rc<HashMap<usize, String>>>,
    /// The note the rendering happens in (vault path, its current text): code-block renderers
    /// that list the note's contents use it.
    note: Option<Rc<(String, Rc<str>)>>,
}

impl MarkdownRenderer {
    pub fn new(resolve: Rc<dyn Fn(&str) -> Option<PathBuf>>) -> Self {
        Self { resolve, depth: 0, next: Rc::default(), numbers: None, note: None }
    }

    /// The note this renders in (vault path, current text).
    pub fn with_note(mut self, path: String, text: Rc<str>) -> Self {
        self.note = Some(Rc::new((path, text)));
        self
    }

    /// The note this renders in: (vault path, current text), if known.
    pub fn note(&self) -> Option<(&str, &str)> {
        self.note.as_ref().map(|n| (n.0.as_str(), &*n.1))
    }

    /// Callout numbers for the text about to be rendered (by its line numbers).
    pub fn with_numbers(mut self, numbers: HashMap<usize, String>) -> Self {
        self.numbers = Some(Rc::new(numbers));
        self
    }

    pub fn render(&self, md: &str, window: &mut Window, cx: &mut App) -> AnyElement {
        let n = self.next.get();
        self.next.set(n + 1);
        let el = self.render_inner(md, window, cx);
        div().id(SharedString::from(format!("md-{}-{n}", self.depth))).child(el).into_any_element()
    }

    fn render_inner(&self, md: &str, window: &mut Window, cx: &mut App) -> AnyElement {
        if self.depth > 4 {
            return div().child(md.to_string()).into_any_element(); // runaway nesting
        }
        let host = cx.try_global::<PluginHost>();
        let code_mode = |lang: &str| host.map_or(CodeMode::Plain, |h| h.code_mode(lang));
        let resolve = |t: &str| (self.resolve)(t);
        // the note's cssclasses (a preview of another note, or nested Markdown in this one)
        let classes = self
            .note
            .as_ref()
            .map(|n| {
                let t: &str = &n.1;
                let mut cut = t.len().min(8192);
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                tephra_core::index::cssclasses(&t[..cut])
            })
            .unwrap_or_default();
        let (style, body, highlight) = crate::theme::text_style(cx, &classes);
        let proj = project(&ProjectionInput {
            text: md,
            sel: Selection::caret(0),
            mode: ViewMode::Reading,
            version: 0,
            palette: Palette { highlight },
            resolve: &resolve,
            link_label: &|_| None,
            code_mode: &code_mode,
            // a plugin block's nested Markdown must not reuse the editor's leaf keys, or its text
            // would overwrite the editor's leaf geometry (first line unclickable, caret jumps)
            key_base: super::projection::NESTED_KEY_BASE,
        });
        let muted = cx.theme().muted_foreground;
        let mut rcx = RenderContext::from_app(cx).with_style(style).with_markdown_extensions(crate::atoms::extensions()).with_body_font_size(px(body));
        let mut fg: HashMap<u64, Vec<(std::ops::Range<usize>, Hsla)>> = proj.colors.clone();
        for (k, ranges) in &proj.muted {
            fg.entry(*k).or_default().extend(ranges.iter().map(|r| (r.clone(), muted)));
        }
        for (k, ranges) in fg {
            rcx = rcx.with_foreground_ranges(k, ranges);
        }
        let inner = MarkdownRenderer { resolve: self.resolve.clone(), depth: self.depth + 1, next: Rc::default(), numbers: None, note: self.note.clone() };
        let folds = HashMap::new();
        let h = Host { editor: None, folds: &folds, md: &inner, numbers: self.numbers.as_deref(), classes: &classes };
        let children: Vec<AnyElement> = proj
            .blocks
            .iter()
            .enumerate()
            .map(|(ix, b)| {
                let el = render_node(&b.node, ix, &h, &rcx, window, cx);
                if b.indent > 0.0 { div().pl(crate::ui::r(b.indent)).child(el).into_any_element() } else { el }
            })
            .collect();
        div().flex().flex_col().min_w_0().children(children).into_any_element()
    }
}

/// What node rendering needs from where it happens: the editor (folds, caret on click), or
/// nothing (read-only Markdown inside a plugin block).
pub struct Host<'a> {
    pub editor: Option<WeakEntity<Editor>>,
    pub folds: &'a HashMap<String, bool>,
    pub md: &'a MarkdownRenderer,
    /// Plugin callout numbers for this note, by header line.
    pub numbers: Option<&'a HashMap<usize, String>>,
    /// The note's `cssclasses` (theme variables scoped by class).
    pub classes: &'a [String],
}

/// Renders a projected node; gpui-kit renders plain blocks.
pub fn render_node(node: &ProjNode, ix: usize, host: &Host, rcx: &RenderContext, window: &mut Window, cx: &mut App) -> AnyElement {
    match node {
        // drawn here, not by gpui-kit, so table border settings (`.NoBorder`) leave rules alone
        ProjNode::Block(BlockNode::HorizontalRule { .. }) => {
            let (color, w) = crate::theme::hr(cx, host.classes);
            div().w_full().py_3().child(div().w_full().h(px(w.max(0.5))).bg(color)).into_any_element()
        }
        ProjNode::Block(b) => b.render(ix, FlowPosition::at(&[b], 0, true).first(true).last(true), rcx, window, cx),
        ProjNode::Callout(c) => render_callout(c, ix, host, rcx, window, cx),
        ProjNode::Display { node: b, line } => {
            let el = b.render(ix, FlowPosition::at(&[b], 0, true).first(true).last(true), rcx, window, cx);
            let centred = crate::app_settings::AppSettings::flag(cx, "centerDisplayMath");
            // a plugin's equation number ("(1.2)") at the right, as LaTeX does
            match host.numbers.and_then(|m| m.get(line)) {
                Some(tag) => div()
                    .w_full()
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w_0().flex().when(centred, |d| d.justify_center()).child(el))
                    .child(div().flex_none().pl_4().child(tag.clone()))
                    .into_any_element(),
                None if centred => div().w_full().flex().justify_center().child(el).into_any_element(),
                None => el,
            }
        }
        ProjNode::Noted { note, child } => {
            let muted = cx.theme().muted_foreground;
            let el = child.render(ix, FlowPosition::at(&[child], 0, true).first(true).last(true), rcx, window, cx);
            div().flex().flex_col().child(div().text_xs().text_color(muted).child(note.clone())).child(el).into_any_element()
        }
        ProjNode::Task { checked, at, child } => {
            let el = child.render(ix, FlowPosition::at(&[child], 0, true).first(true).last(true), rcx, window, cx);
            let theme = cx.theme();
            let (accent, border) = (theme.primary, theme.muted_foreground);
            let (checked, at) = (*checked, at.clone());
            let editor = host.editor.clone();
            let mut tick = div()
                .id(SharedString::from(format!("task-{ix}-{}", at.start)))
                .flex_none()
                .mt(px(5.))
                .size(px(15.))
                .rounded(px(3.))
                .border_1()
                .flex()
                .items_center()
                .justify_center()
                ;
            tick = if checked { {
                use gpui_kit::component::Sizable as _;
                tick.bg(accent).border_color(accent).child(gpui_kit::component::Icon::new(IconName::Check).xsmall().text_color(theme.primary_foreground))
            } } else { tick.border_color(border) };
            if let Some(editor) = editor {
                tick = tick.cursor_pointer().on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let _ = editor.update(cx, |e, cx| e.toggle_task(at.clone(), cx));
                });
            }
            div().flex().items_start().gap_2().child(tick).child(div().flex_1().min_w_0().child(el)).into_any_element()
        }
        ProjNode::Stack(items) => div()
            .flex()
            .flex_col()
            .children(items.iter().map(|(indent, n)| {
                let el = render_node(n, ix, host, rcx, window, cx);
                if *indent > 0.0 { div().pl(crate::ui::r(*indent)).child(el).into_any_element() } else { el }
            }))
            .into_any_element(),
        ProjNode::Rendered { lang, code, at } => {
            let renderer = cx.try_global::<PluginHost>().and_then(|h| h.reg.code_block_renderer(lang));
            let inner = match renderer {
                Some(r) => r.render(code, host.md, window, cx),
                None => div().child(code.clone()).into_any_element(),
            };
            let at = *at;
            let Some(editor) = host.editor.clone() else { return inner };
            // clicking a rendered block puts the caret in it, which reveals its source
            div()
                .id(SharedString::from(format!("rendered-{ix}-{at}")))
                .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, cx| {
                    let _ = editor.update(cx, |e, cx| {
                        window.focus(&e.focus, cx);
                        e.set_selection(Selection::caret(at), false, cx);
                    });
                    cx.stop_propagation();
                })
                .child(inner)
                .into_any_element()
        }
    }
}

fn render_callout(c: &CalloutView, ix: usize, host: &Host, rcx: &RenderContext, window: &mut Window, cx: &mut App) -> AnyElement {
    let folds = host.folds;
    let style = callout_style(&c.kind, &c.meta, cx);
    let color = style.color;
    let plugin = cx.try_global::<PluginHost>().and_then(|h| h.reg.callout_renderer(&c.kind, &c.meta));
    // a number from the plugin ("Theorem 1.2") replaces the plain type label
    let mut style = style;
    if let Some(n) = host.numbers.and_then(|m| m.get(&c.line)) {
        style.label = Some(n.clone());
    }
    let collapsed = c.fold.is_some() && folds.get(&c.fold_key).copied().unwrap_or(c.fold == Some('-'));
    let title = BlockNode::Paragraph(c.title.clone());
    let title_el = title.render(ix, FlowPosition::at(&[&title], 0, true).first(true).last(true), rcx, window, cx);
    let fold_btn = c.fold.zip(host.editor.clone()).map(|(_, editor)| {
        let key = c.fold_key.clone();
        let default = c.fold == Some('-');
        div()
            .id(SharedString::from(format!("fold-{}", c.fold_key)))
            .cursor_pointer()
            .text_color(color)
            .child(if collapsed { "▸" } else { "▾" })
            .on_click(move |_: &ClickEvent, _, cx| {
                let _ = editor.update(cx, |e, cx| {
                    let cur = e.folds.get(&key).copied().unwrap_or(default);
                    e.folds.insert(key.clone(), !cur);
                    cx.notify();
                });
            })
    });
    let custom = if collapsed { None } else { plugin.as_ref().and_then(|p| p.render_body(&c.kind, &c.meta, &c.body, host.md, window, cx)) };
    let mut body: Vec<AnyElement> = match custom {
        // a plugin-drawn body: clicking it puts the caret in the callout (its source shows)
        Some(el) => match host.editor.clone() {
            Some(editor) => {
                let at = c.body_start;
                vec![div()
                    .id(SharedString::from(format!("callout-body-{}", c.fold_key)))
                    .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, cx| {
                        let _ = editor.update(cx, |e, cx| {
                            window.focus(&e.focus, cx);
                            e.set_selection(Selection::caret(at), false, cx);
                        });
                        cx.stop_propagation();
                    })
                    .child(el)
                    .into_any_element()]
            }
            None => vec![el],
        },
        None if collapsed => Vec::new(),
        None => c.children.iter().map(|ch| render_node(ch, ix, host, rcx, window, cx)).collect(),
    };
    if let Some(end) = (!collapsed).then(|| plugin.as_ref().and_then(|p| p.body_end(&c.kind))).flatten() {
        body.push(div().flex().justify_end().text_color(color).child(end).into_any_element());
    }
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
                .child(div().italic().font_weight(FontWeight::SEMIBOLD).text_color(color).child(label))
                .child(div().flex_1().min_w_0().italic().child(title_el))
        }
        CalloutLook::Caption { .. } => {
            // "Figure 1.2  Caption": bold label, then the title (or nothing), centred
            let label = style.label.clone().unwrap_or_default();
            h_flex()
                .gap_2()
                .items_center()
                .justify_center()
                .children(fold_btn)
                .when(!label.is_empty(), |d| d.child(div().font_weight(FontWeight::BOLD).child(label)))
                .child(div().min_w_0().child(title_el))
        }
        CalloutLook::Badge | CalloutLook::Rules => {
            let label = style.label.clone().unwrap_or_else(|| capitalize(&c.kind));
            let badge = if style.look == CalloutLook::Badge {
                div().px_2().py_0p5().rounded(px(4.)).bg(color).text_color(on_color(color)).text_sm().font_weight(FontWeight::SEMIBOLD).child(label)
            } else {
                div().font_weight(FontWeight::BOLD).text_color(color).child(label)
            };
            h_flex().gap_2().items_center().children(fold_btn).child(badge).child(div().flex_1().min_w_0().child(title_el))
        }
    };
    if let CalloutLook::Caption { below } = style.look {
        let body = div().flex().flex_col().items_center().children(body);
        let el = div().my_2().flex().flex_col().gap_1().w_full();
        let el = if below { el.child(body).child(header) } else { el.child(header).child(body) };
        return el.into_any_element();
    }
    let el = div().my_1().flex().flex_col().gap_1().child(header).children(body);
    let el = match style.look {
        CalloutLook::Obsidian => el.pl(crate::ui::r(24.)).pr(crate::ui::r(12.)).py(crate::ui::r(12.)).rounded(px(4.)).bg(color.opacity(0.1)),
        CalloutLook::Badge => el.px_3().pt_2().pb_2().border(px(1.4)).border_color(color).rounded(px(6.)).bg(color.opacity(0.06)),
        CalloutLook::Soft => el.pl_3().border_l_2().border_color(color.opacity(0.6)),
        CalloutLook::Rules => el.px_2().py_1().border_t_2().border_b_2().border_color(color),
        CalloutLook::Caption { .. } => el,
    };
    el.into_any_element()
}

#[cfg(test)]
mod highlight_tests {
    use gpui_kit::base::input::{HighlightStyleResolver, Rope};
    use gpui_kit::component::highlighter::SyntaxHighlighter;
    use gpui_kit::HighlightStyle;

    /// Styles only keywords, so a styled range means "highlighted as a keyword".
    struct Keywords;
    impl HighlightStyleResolver for Keywords {
        fn style(&self, name: &str) -> Option<HighlightStyle> {
            (name == "keyword").then(|| HighlightStyle { color: Some(gpui_kit::red()), ..Default::default() })
        }
    }

    #[test]
    fn cpp_gets_cs_highlight_rules_too() {
        super::register_languages();
        let code = "int main() { return 0; }";
        let mut h = SyntaxHighlighter::new("cpp");
        h.update(None, &Rope::from_str(code), None);
        let keywords: Vec<&str> = h.styles(&(0..code.len()), &Keywords).into_iter().filter(|(_, s)| s.color.is_some()).map(|(r, _)| &code[r]).collect();
        assert!(keywords.contains(&"return"), "C's keywords in C++: {keywords:?}");
    }
}
