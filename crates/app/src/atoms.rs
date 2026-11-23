//! Renderers for the projection's atoms (math formulas, image embeds), registered with
//! gpui-kit as Markdown plugins. The projection builds the nodes directly, so `parse`
//! never claims anything from gpui-kit's own Markdown parser.

use std::sync::Arc;

use gpui_kit::component::text::{markdown_ast, InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use crate::editor::projection::{ImageAtom, MathAtom, SupSubAtom, IMAGE_NODE, MATH_NODE, SUPSUB_NODE};

/// Widest an embedded image is drawn, in logical pixels.
pub const MAX_IMAGE_WIDTH: f32 = 680.0;

thread_local! {
    /// Width available to the editor being rendered (narrow panes shrink images to fit).
    pub static CONTENT_WIDTH: std::cell::Cell<f32> = const { std::cell::Cell::new(MAX_IMAGE_WIDTH) };
}

pub fn extensions() -> Arc<MarkdownExtensions> {
    Arc::new(MarkdownExtensions::default().plugin(MathPlugin).plugin(ImagePlugin).plugin(SupSubPlugin))
}

struct MathPlugin;

impl MarkdownPlugin for MathPlugin {
    fn name(&self) -> &str {
        MATH_NODE
    }
    fn parse(&self, _: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        None
    }
    fn render_inline(&self, node: &MarkdownNode, context: &InlineRenderContext, window: &mut Window, cx: &mut App) -> Option<InlineElement> {
        let m = node.data::<MathAtom>()?;
        let font_px = f32::from(context.font_size()) * if m.display { 1.1 } else { 1.0 };
        let color = context.text_style().color;
        match crate::math::render(&m.tex, m.display, font_px, color, window.scale_factor()) {
            Some(img_) => {
                let el = img(img_.image).w(px(img_.width)).h(px(img_.height));
                if m.display {
                    // display math: its own line, a little air above and below; optionally centred
                    let centred = crate::app_settings::AppSettings::flag(cx, "centerDisplayMath");
                    let d = if centred { div() } else { div().pl_4() };
                    Some(InlineElement::new(d.py_1().child(el)).with_baseline(px(img_.baseline + 4.0)))
                } else {
                    Some(InlineElement::new(el).with_baseline(px(img_.baseline)))
                }
            }
            None => Some(InlineElement::new(
                div().text_color(cx.theme().danger).child(SharedString::from(if m.display { format!("$${}$$", m.tex) } else { format!("${}$", m.tex) })),
            )),
        }
    }
}

struct ImagePlugin;

impl MarkdownPlugin for ImagePlugin {
    fn name(&self) -> &str {
        IMAGE_NODE
    }
    fn parse(&self, _: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        None
    }
    fn render_inline(&self, node: &MarkdownNode, _: &InlineRenderContext, _: &mut Window, cx: &mut App) -> Option<InlineElement> {
        let a = node.data::<ImageAtom>()?;
        let Some(path) = a.path.clone() else {
            return Some(InlineElement::new(div().text_color(cx.theme().danger).child(SharedString::from(format!("![[{}]] — not found", a.name)))));
        };
        // images grow with the UI scale, as with Obsidian's zoom, up to the pane's width
        let scale = crate::theme::ui_scale(cx);
        let (w, h) = a.size.map(|(w, h)| (w as f32 * scale, h as f32 * scale)).unwrap_or((320.0 * scale, 200.0 * scale));
        let max = CONTENT_WIDTH.with(|c| c.get()).clamp(120.0, MAX_IMAGE_WIDTH * scale);
        let scale = (max / w).min(1.0);
        // whole pixels: gpui-kit rejects an object whose baseline exceeds its measured height
        let (w, h) = ((w * scale).round(), (h * scale).round());
        // decoded lazily by GPUI's image cache when first painted
        Some(InlineElement::new(img(path).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)).with_baseline(px(h)))
    }
}

/// `<sup>` / `<sub>`: three-quarter size text, raised or lowered (GPUI text has no baseline
/// shift, so it is an inline object whose baseline is placed to move it).
struct SupSubPlugin;

impl MarkdownPlugin for SupSubPlugin {
    fn name(&self) -> &str {
        SUPSUB_NODE
    }
    fn parse(&self, _: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        None
    }
    fn render_inline(&self, node: &MarkdownNode, context: &InlineRenderContext, window: &mut Window, _: &mut App) -> Option<InlineElement> {
        let a = node.data::<SupSubAtom>()?;
        let style = context.text_style();
        let fs = f32::from(context.font_size()) * 0.75;
        let h = (fs * 1.3).round();
        let run = TextRun { len: a.text.len(), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
        let w = window.text_system().shape_line(a.text.clone().into(), px(fs), &[run], None).width;
        let el = div().w(w.ceil() + px(1.)).h(px(h)).text_size(px(fs)).line_height(px(h)).text_color(style.color).child(SharedString::from(a.text.clone()));
        // the object's baseline: at its bottom, the small text sits a third of a line above the
        // text's baseline; near its top, it hangs below
        let baseline = if a.sup { h } else { (h * 0.45).round() };
        Some(InlineElement::new(el).with_baseline(px(baseline)))
    }
}
