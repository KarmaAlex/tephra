//! obsidilight: a lightweight, Obsidian-compatible notes app.
//!
//! Opens a vault (file tree + tabs), optionally with one note open:
//!   obsidilight <vault-folder | path/to/note.md>
//! Env: OBSIDILIGHT_BENCH=1 runs the typing benchmark, prints `REPORT {json}` and exits.
//!      OBSIDILIGHT_SCRIPT=<file> replays scripted input (see script.rs).

mod app_settings;
mod atoms;
mod bench;
mod commands;
mod editor;
mod embeds;
mod export;
mod fuzzy;
mod hotkeys;
mod icons;
mod math;
mod plugins;
mod script;
mod settings_ui;
mod workspace;

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::*;

fn main() {
    let t0 = Instant::now();
    // PDF export runs in a short-lived copy of this binary (see export::compile::run_isolated)
    if std::env::args_os().nth(1).is_some_and(|a| a == "--export-worker") {
        if let Some(dir) = std::env::args_os().nth(2) {
            export::compile::worker(std::path::Path::new(&dir));
        }
        return;
    }
    let Some(arg) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: obsidilight <vault-folder | note.md>");
        std::process::exit(2);
    };
    let arg = std::fs::canonicalize(&arg).unwrap_or(arg);
    let root = embeds::Vault::root_for(&arg);
    let note = arg.is_file().then(|| arg.clone());
    let title: SharedString = root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().into();
    gpui_kit::application().with_assets(icons::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        editor::bind_keys(cx);
        workspace::bind_keys(cx);
        app_settings::AppSettings::init(cx, &root);
        plugins::PluginHost::init(cx, &root);
        hotkeys::install(cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(860.)), cx))),
            titlebar: Some(TitlebarOptions { title: Some(title), ..Default::default() }),
            app_id: Some("obsidilight".into()),
            ..Default::default()
        };
        let result = gpui_kit::open_window(opts, cx, move |window, cx| {
            let ws = cx.new(|cx| workspace::Workspace::new(root, window, cx));
            window.on_next_frame(move |_, _| println!("READY {:.1}", t0.elapsed().as_secs_f64() * 1000.0));
            if let Some(note) = note {
                let editor = ws.update(cx, |ws, cx| ws.open_path(note, false, window, cx));
                script::start(editor.clone(), ws.downgrade(), window, cx);
                if std::env::var_os("OBSIDILIGHT_BENCH").is_some() {
                    bench::start(editor, window, cx);
                }
            }
            ws
        });
        if let Err(e) = result {
            eprintln!("could not open window: {e}");
            std::process::exit(1);
        }
    });
}
