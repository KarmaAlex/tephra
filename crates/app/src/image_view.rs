//! An image opened on its own (clicking a `.png` in Files, a graph node, an image link): shown
//! whole, centred, at its natural size or shrunk to fit the pane, as Obsidian does.

use std::path::PathBuf;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

pub const VIEW: &str = "app:image";

pub struct ImageView {
    path: PathBuf,
    focus: FocusHandle,
}

impl ImageView {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        Self { path, focus: cx.focus_handle() }
    }
}

impl Focusable for ImageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let scale = crate::theme::ui_scale(cx);
        let body = if !self.path.exists() {
            div().text_color(theme.muted_foreground).child(format!("{} — not found", self.path.display())).into_any_element()
        } else {
            let (w, h) = crate::embeds::image_size(&self.path).map(|(w, h)| (w as f32 * scale, h as f32 * scale)).unwrap_or((640.0, 480.0));
            img(self.path.clone()).w(px(w)).h(px(h)).max_w_full().max_h_full().object_fit(ObjectFit::Contain).into_any_element()
        };
        div().id("image-view").track_focus(&self.focus).size_full().p_4().flex().items_center().justify_center().bg(theme.background).child(body)
    }
}
