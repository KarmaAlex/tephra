//! Shared UI pieces sized like Obsidian's at 100% (measured on Obsidian 1.15, M6 feedback):
//! 16 px icons in 28×24 buttons, 25 px list rows with 13 px text, a 44 px ribbon, 300 px
//! sidebars. Sizes are in rems, so the UI scale (Settings → Appearance) scales them all.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::*;

/// Obsidian's pixel size at 100% as rems (1 rem = 16 px at 100%).
pub fn r(px_at_100: f32) -> Rems {
    rems(px_at_100 / 16.0)
}

/// An icon button like Obsidian's (ribbon, sidebar headers, toolbars).
pub fn icon_button(id: impl Into<ElementId>, icon: impl Into<gpui_kit::component::Icon>) -> Button {
    Button::new(id).ghost().icon(icon).w(r(28.)).h(r(24.))
}

/// Obsidian's pixel size at 100%, scaled (for APIs that take pixels, such as dialog widths).
pub fn p(cx: &App, px_at_100: f32) -> Pixels {
    px(px_at_100 * crate::theme::ui_scale(cx))
}

/// Ribbon width and sidebar width.
pub const RIBBON: f32 = 44.0;
pub const SIDEBAR: f32 = 300.0;
