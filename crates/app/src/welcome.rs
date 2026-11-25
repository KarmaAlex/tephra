//! The welcome page (Obsidian's vault manager): shown when the app starts without an argument
//! and no last vault to reopen, and by "Open vault manager". Recent vaults on the left; on the
//! right, create a vault, open a folder as a vault, or open a folder.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::embeds::VaultRoot;
use crate::ui::r;
use crate::vaults::{self, Known, OpenAs};

pub struct Welcome {
    known: Known,
    focus: FocusHandle,
}

/// Windows showing the welcome page.
#[derive(Default)]
struct WelcomeWindows(HashSet<WindowId>);
impl Global for WelcomeWindows {}

pub fn is_welcome(window: &Window, cx: &App) -> bool {
    cx.try_global::<WelcomeWindows>().is_some_and(|w| w.0.contains(&window.window_handle().window_id()))
}

/// Opens a welcome window.
pub fn open(cx: &mut App) {
    if !cx.has_global::<crate::theme::ThemeCss>() {
        crate::theme::init_default(cx);
    }
    crate::theme::apply(None, cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(860.), px(580.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some("Tephra".into()), ..Default::default() }),
        app_id: Some("tephra".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(opts, cx, |window, cx| {
        window.observe_window_appearance(|window, cx| crate::theme::apply(Some(window), cx)).detach();
        crate::theme::apply(Some(window), cx);
        cx.default_global::<WelcomeWindows>().0.insert(window.window_handle().window_id());
        let view = cx.new(|cx| Welcome { known: Known::load(), focus: cx.focus_handle() });
        window.focus(&view.read(cx).focus.clone(), cx);
        crate::script::start(None, None, window, cx);
        view
    });
    if let Err(e) = result {
        eprintln!("could not open window: {e}");
        std::process::exit(1);
    }
}

impl Focusable for Welcome {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Wraps `el` so scripted UI tests can find it under `key`.
fn probed(key: &'static str, el: impl IntoElement, cx: &App) -> Div {
    div().relative().child(el).children(crate::script::probe(|| key.to_string(), cx))
}

impl Welcome {
    fn render_recent(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let current = cx.try_global::<VaultRoot>().map(|r| r.0.clone());
        // vaults deleted or moved since are left out (still remembered, in case they come back)
        let recent: Vec<PathBuf> = self.known.recent.iter().filter(|p| p.is_dir()).cloned().collect();
        let rows = recent.into_iter().enumerate().map(|(i, path)| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
            let is_current = current.as_ref() == Some(&path);
            let (open_path, forget_path) = (path.clone(), path.clone());
            let key = format!("welcome:recent:{name}");
            h_flex()
                .id(("welcome-recent", i))
                .relative()
                .group("welcome-recent")
                .gap_1()
                .pl_3()
                .pr_1()
                .py(r(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(is_current, |d| d.bg(theme.muted))
                .hover(|s| s.bg(theme.muted))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_size(r(14.)).font_weight(FontWeight::MEDIUM).truncate().child(name))
                        .child(div().text_size(r(12.)).text_color(theme.muted_foreground).truncate().child(path.display().to_string())),
                )
                .child(
                    crate::ui::icon_button(("welcome-forget", i), IconName::Close).xsmall().tooltip("Remove from list").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        vaults::forget(&forget_path);
                        this.known.forget(&forget_path);
                        cx.notify();
                    })),
                )
                .on_click(move |_, window, cx| vaults::open(open_path.clone(), OpenAs::Folder, window, cx))
                .children(crate::script::probe(move || key, cx))
        });
        let rows: Vec<_> = rows.collect();
        let empty = rows.is_empty();
        v_flex()
            .id("welcome-recent-list")
            .w(r(280.))
            .h_full()
            .flex_none()
            .gap_0p5()
            .p_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .children(rows)
            .when(empty, |d| d.child(div().p_3().text_size(r(13.)).text_color(theme.muted_foreground).child("No recent vaults")))
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let row = |title: &'static str, desc: &'static str, button: AnyElement, last: bool| {
            h_flex()
                .gap_4()
                .py_3()
                .when(!last, |d| d.border_b_1().border_color(theme.border))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().text_size(r(14.)).font_weight(FontWeight::MEDIUM).child(title))
                        .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(desc)),
                )
                .child(div().flex_none().child(button))
        };
        let create = probed("welcome:create", Button::new("welcome-create").label("Create").primary().on_click(|_, window, cx| vaults::create_vault_dialog(window, cx)), cx).into_any_element();
        let as_vault = probed("welcome:open-as-vault", Button::new("welcome-open-as-vault").label("Open").outline().on_click(|_, window, cx| vaults::pick_and_open(OpenAs::Vault, window, cx)), cx).into_any_element();
        let folder = probed("welcome:open-folder", Button::new("welcome-open-folder").label("Open").outline().on_click(|_, window, cx| vaults::pick_and_open(OpenAs::Folder, window, cx)), cx).into_any_element();
        let actions = v_flex()
            .w(r(460.))
            .child(row("Create new vault", "Create a new vault in a folder of your choice.", create, false))
            .child(row("Open folder as vault", "Choose an existing folder of Markdown files. It gets an empty .obsidian folder if it has none.", as_vault, false))
            .child(row("Open folder", "Open a folder as it is. A folder inside a vault opens that vault.", folder, true));
        let main = v_flex()
            .flex_1()
            .h_full()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(img(crate::icons::LOGO).w(r(72.)).h(r(72.)))
                    .child(div().text_size(r(26.)).font_weight(FontWeight::SEMIBOLD).child("Tephra"))
                    .child(div().text_size(r(13.)).text_color(theme.muted_foreground).child(concat!("Version ", env!("CARGO_PKG_VERSION")))),
            )
            .child(actions);
        h_flex()
            .id("welcome")
            .key_context("Welcome")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_recent(cx))
            .child(main)
    }
}
