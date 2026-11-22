//! obsidilight: a lightweight, Obsidian-compatible notes app.
//!
//! M1a: opens one note in the live-preview editor.
//!   obsidilight <path/to/note.md>
//! Env: OBSIDILIGHT_BENCH=1 runs the typing benchmark, prints `REPORT {json}` and exits.
//!      OBSIDILIGHT_SCRIPT=<file> replays scripted input (see script.rs).

mod atoms;
mod bench;
mod editor;
mod embeds;
mod math;
mod script;

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::*;

fn main() {
    let t0 = Instant::now();
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: obsidilight <note.md>");
        std::process::exit(2);
    };
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        editor::bind_keys(cx);
        open_note(path, cx, move |editor, window, cx| {
            window.on_next_frame(move |_, _| println!("READY {:.1}", t0.elapsed().as_secs_f64() * 1000.0));
            script::start(editor.clone(), window, cx);
            if std::env::var_os("OBSIDILIGHT_BENCH").is_some() {
                bench::start(editor, window, cx);
            }
        });
    });
}

/// Opens a note in a new window (one note per window until tabs arrive in M1b).
pub fn open_note(path: PathBuf, cx: &mut App, setup: impl FnOnce(Entity<editor::Editor>, &mut Window, &mut App) + 'static) {
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let title: SharedString = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().into();
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1000.), px(860.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some(title), ..Default::default() }),
        app_id: Some("obsidilight".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(opts, cx, move |window, cx| {
        let editor = cx.new(|cx| editor::Editor::open(path, window, cx));
        setup(editor.clone(), window, cx);
        editor
    });
    if let Err(e) = result {
        eprintln!("could not open window: {e}");
    }
}
