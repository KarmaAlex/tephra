//! Warnings, errors and panics from the app and GPUI (`log` crate): to stderr, and on Windows
//! also to `%LOCALAPPDATA%\obsidilight\obsidilight.log` -- a release .exe is a GUI-subsystem
//! app with no console, so without the file they would be lost (e.g. GPUI's image load errors).
//! The file is rewritten on every start. `OBSIDILIGHT_LOG=<file>` picks another file (any OS);
//! `OBSIDILIGHT_LOG_LEVEL=info|debug|trace` logs more.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

struct Logger {
    file: Option<Mutex<std::fs::File>>,
}

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{:<5} {}: {}\n", record.level(), record.target(), record.args());
        eprint!("{line}");
        if let Some(f) = &self.file {
            if let Ok(mut f) = f.lock() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {}
}

/// Where the log file goes, if anywhere.
pub fn file_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OBSIDILIGHT_LOG") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_absolute())?;
        return Some(base.join("obsidilight").join("obsidilight.log"));
    }
    None
}

/// Installs the logger and a panic hook that logs the panic before the default report.
pub fn init() {
    let file = file_path().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::File::create(&p).ok().map(Mutex::new)
    });
    let level = match std::env::var("OBSIDILIGHT_LOG_LEVEL").as_deref() {
        Ok("trace") => log::LevelFilter::Trace,
        Ok("debug") => log::LevelFilter::Debug,
        Ok("info") => log::LevelFilter::Info,
        _ => log::LevelFilter::Warn,
    };
    if log::set_boxed_logger(Box::new(Logger { file })).is_ok() {
        log::set_max_level(level);
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}\n{}", std::backtrace::Backtrace::force_capture());
        default(info);
    }));
}
