//! obsidilight: a lightweight, Obsidian-compatible notes app.
//!
//! Opens a vault (file tree + tabs), optionally with one note open:
//!   obsidilight <vault-folder | path/to/note.md>
//! Env: OBSIDILIGHT_BENCH=1 runs the typing benchmark, prints `REPORT {json}` and exits.
//!      OBSIDILIGHT_SCRIPT=<file> replays scripted input (see script.rs).
//! Without an argument (app menu, double-clicked .exe) the last opened vault opens again; when
//! there is none (first start, or it was deleted), the welcome page (see welcome.rs).

// Release builds on Windows are GUI-subsystem apps: no console window behind the editor.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app_settings;
mod atoms;
mod bench;
mod commands;
mod css;
mod editor;
mod embeds;
mod export;
mod file_drag;
mod fonts;
mod fuzzy;
mod hotkeys;
mod image_view;
mod logging;
mod icons;
mod math;
mod moment;
mod plugins;
mod script;
mod settings_ui;
mod theme;
mod ui;
mod vaults;
mod watcher;
mod welcome;
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
    if std::env::args_os().nth(1).is_some_and(|a| a == "--help" || a == "-h") {
        println!("usage: obsidilight [vault-folder | note.md]\n(without an argument the last opened vault, else the welcome page)");
        return;
    }
    logging::init();
    let arg = std::env::args_os().nth(1).map(PathBuf::from);
    gpui_kit::application().with_assets(icons::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        fonts::load(cx);
        editor::render::register_languages();
        editor::bind_keys(cx);
        workspace::bind_keys(cx);
        match arg {
            Some(arg) => open_vault(arg, t0, cx),
            // started from the application menu / by double-clicking the .exe: the last vault
            None => match vaults::Known::load().startup_vault() {
                Some(last) => open_vault(last, t0, cx),
                None => welcome::open(cx),
            },
        }
    });
}

/// Opens the main window on a vault folder, or on the vault containing a note (which is opened).
fn open_vault(arg: PathBuf, t0: Instant, cx: &mut App) {
    let arg = std::fs::canonicalize(&arg).unwrap_or(arg);
    let root = embeds::Vault::root_for(&arg);
    let note = arg.is_file().then(|| arg.clone());
    if root.is_dir() {
        vaults::record_opened(&root);
    }
    let title: SharedString = root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().into();
    app_settings::AppSettings::init(cx, &root);
    plugins::PluginHost::init(cx, &root);
    hotkeys::install(cx);
    cx.set_global(embeds::VaultRoot(root.clone()));
    theme::init(cx, &root);
    theme::apply(None, cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(860.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some(title), ..Default::default() }),
        app_id: Some("obsidilight".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(opts, cx, move |window, cx| {
        // follow the system's light/dark switch (when the base theme is "system")
        window.observe_window_appearance(|window, cx| theme::apply(Some(window), cx)).detach();
        theme::apply(Some(window), cx);
        let ws = cx.new(|cx| workspace::Workspace::new(root, window, cx));
        window.on_next_frame(move |_, _| println!("READY {:.1}", t0.elapsed().as_secs_f64() * 1000.0));
        let editor = note.map(|note| ws.update(cx, |ws, cx| ws.open_path(note, false, window, cx)));
        script::start(editor.clone(), Some(ws.downgrade()), window, cx);
        if let Some(editor) = editor.filter(|_| std::env::var_os("OBSIDILIGHT_BENCH").is_some()) {
            bench::start(editor, window, cx);
        }
        ws
    });
    if let Err(e) = result {
        eprintln!("could not open window: {e}");
        std::process::exit(1);
    }
}
