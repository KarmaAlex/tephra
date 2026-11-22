//! Renderers for the projection's atoms (math formulas, image embeds), registered with
//! gpui-kit as Markdown plugins. The projection builds the nodes directly, so `parse`
//! never claims anything from gpui-kit's own Markdown parser.

use std::sync::Arc;

use gpui_kit::component::text::{markdown_ast, InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use crate::editor::projection::{ImageAtom, MathAtom, IMAGE_NODE, MATH_NODE};

/// Widest an embedded image is drawn, in logical pixels.
pub const MAX_IMAGE_WIDTH: f32 = 680.0;

pub fn extensions() -> Arc<MarkdownExtensions> {
    Arc::new(MarkdownExtensions::default().plugin(MathPlugin).plugin(ImagePlugin))
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
                    // display math: its own line, a little air above and below
                    Some(InlineElement::new(div().py_1().pl_4().child(el)).with_baseline(px(img_.baseline + 4.0)))
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
        let (w, h) = a.size.map(|(w, h)| (w as f32, h as f32)).unwrap_or((320.0, 200.0));
        let scale = (MAX_IMAGE_WIDTH / w).min(1.0);
        // whole pixels: gpui-kit rejects an object whose baseline exceeds its measured height
        let (w, h) = ((w * scale).round(), (h * scale).round());
        // decoded lazily by GPUI's image cache when first painted
        Some(InlineElement::new(img(path).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)).with_baseline(px(h)))
    }
}
