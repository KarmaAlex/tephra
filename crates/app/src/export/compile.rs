//! Typesetting an exported note with Typst, in-process (the M0 spike's approach): formulas go
//! through MathJax (SVG, the engine with no failures on your corpus), then Typst compiles the
//! document to PDF, plus one PNG per page for the preview. Runs off the UI thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

use super::typst::math_snippet;

/// What to typeset: Typst source with `@@M<n>@@` formula placeholders, the formulas, and the
/// files it embeds (virtual name → file on disk).
pub struct Job {
    pub source: String,
    pub math: Vec<(String, bool)>,
    pub files: Vec<(String, PathBuf)>,
}

pub struct Output {
    pub pdf: Vec<u8>,
    /// One PNG per page (the preview), `PREVIEW_PX` wide.
    pub pages: Vec<Vec<u8>>,
    pub warnings: Vec<String>,
    /// Timings: math, compile, pdf (ms).
    pub ms: (f64, f64, f64),
}

/// Fonts: Typst's embedded ones plus the system's, scanned once (~300 ms the first time).
fn fonts() -> &'static FontStore {
    static F: OnceLock<FontStore> = OnceLock::new();
    F.get_or_init(|| {
        let mut f = FontStore::new();
        f.extend(typst_kit::fonts::embedded());
        f.extend(typst_kit::fonts::system());
        f
    })
}

struct DocWorld {
    main: FileId,
    source: Source,
    files: HashMap<String, Bytes>,
    library: LazyHash<Library>,
}

fn file_id(name: &str) -> FileId {
    FileId::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new(name).expect("virtual path")))
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        fonts().book()
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(id.get().vpath().get_without_slash().into()))
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let name = id.get().vpath().get_without_slash().to_string();
        self.files.get(&name).cloned().ok_or_else(|| FileError::NotFound(name.into()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        fonts().font(index)
    }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        use chrono::Datelike as _;
        let now = chrono::Local::now().date_naive();
        Datetime::from_ymd(now.year(), now.month() as u8, now.day() as u8)
    }
}

/// Runs `job` in a separate process (this binary with `--export-worker <dir>`), so MathJax,
/// Typst's caches and the font index (hundreds of MB) are gone again when it finishes.
pub fn run_isolated(job: Job) -> Result<Output, String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("obsidilight-export-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let res = (|| {
        let spec = serde_json::json!({
            "source": job.source,
            "math": job.math,
            "files": job.files.iter().map(|(n, p)| (n.clone(), p.to_string_lossy().to_string())).collect::<Vec<_>>(),
        });
        std::fs::write(dir.join("job.json"), spec.to_string()).map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let status = std::process::Command::new(exe).arg("--export-worker").arg(&dir).stdin(std::process::Stdio::null()).status().map_err(|e| e.to_string())?;
        let result: serde_json::Value = std::fs::read_to_string(dir.join("result.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| format!("export worker failed ({status})"))?;
        if let Some(e) = result["error"].as_str() {
            return Err(e.to_string());
        }
        let n = result["pages"].as_u64().unwrap_or(0) as usize;
        let pages = (0..n).map(|i| std::fs::read(dir.join(format!("page-{i}.png"))).unwrap_or_default()).collect();
        let ms = |k: &str| result["ms"][k].as_f64().unwrap_or(0.0);
        Ok(Output {
            pdf: std::fs::read(dir.join("out.pdf")).map_err(|e| e.to_string())?,
            pages,
            warnings: result["warnings"].as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect()).unwrap_or_default(),
            ms: (ms("math"), ms("compile"), ms("pdf")),
        })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    res
}

/// The worker side of [`run_isolated`]: `<dir>/job.json` in; `out.pdf`, `page-N.png` and
/// `result.json` out.
pub fn worker(dir: &std::path::Path) {
    let write_result = |v: serde_json::Value| {
        let _ = std::fs::write(dir.join("result.json"), v.to_string());
    };
    let job: Option<Job> = std::fs::read_to_string(dir.join("job.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).map(|v| Job {
        source: v["source"].as_str().unwrap_or("").to_string(),
        math: v["math"].as_array().into_iter().flatten().filter_map(|m| Some((m[0].as_str()?.to_string(), m[1].as_bool()?))).collect(),
        files: v["files"].as_array().into_iter().flatten().filter_map(|f| Some((f[0].as_str()?.to_string(), PathBuf::from(f[1].as_str()?)))).collect(),
    });
    let Some(job) = job else { return write_result(serde_json::json!({ "error": "unreadable export job" })) };
    match run(job) {
        Ok(out) => {
            let _ = std::fs::write(dir.join("out.pdf"), &out.pdf);
            for (i, p) in out.pages.iter().enumerate() {
                let _ = std::fs::write(dir.join(format!("page-{i}.png")), p);
            }
            write_result(serde_json::json!({ "pages": out.pages.len(), "warnings": out.warnings, "ms": { "math": out.ms.0, "compile": out.ms.1, "pdf": out.ms.2 } }));
        }
        Err(e) => write_result(serde_json::json!({ "error": e })),
    }
}

/// Width of the preview images, in pixels (the dialog shows them 560 px wide: sharp on a 2×
/// screen, and small enough that a long note's preview costs tens of MB, not hundreds).
pub const PREVIEW_PX: u32 = 1120;

/// A page's SVG as a PNG of `PREVIEW_PX` width.
fn preview_png(svg: &str) -> Vec<u8> {
    let opt = resvg::usvg::Options::default();
    let Ok(tree) = resvg::usvg::Tree::from_str(svg, &opt) else { return Vec::new() };
    let size = tree.size();
    let scale = PREVIEW_PX as f32 / size.width();
    let h = (size.height() * scale).ceil() as u32;
    let Some(mut pix) = resvg::tiny_skia::Pixmap::new(PREVIEW_PX, h) else { return Vec::new() };
    pix.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    pix.encode_png().unwrap_or_default()
}

/// Typesets the formulas and compiles the document (in this process).
pub fn run(job: Job) -> Result<Output, String> {
    let t0 = Instant::now();
    let mut files: HashMap<String, Bytes> = HashMap::new();
    let mut source = job.source;
    for (i, (tex, display)) in job.math.iter().enumerate().rev() {
        let placeholder = format!("@@M{i}@@");
        let snippet = match crate::math::mathjax_svg(tex, *display) {
            Some(svg) => {
                let svg = svg.replace("currentColor", "#000");
                let name = format!("m{i}.svg");
                let s = math_snippet(&name, &svg, *display);
                files.insert(name, Bytes::new(svg.into_bytes()));
                s
            }
            None => format!("#text(fill: red, raw({}));", super::typst::str_arg(&if *display { format!("$${tex}$$") } else { format!("${tex}$") })),
        };
        source = source.replace(&placeholder, &snippet);
    }
    for (name, path) in &job.files {
        match std::fs::read(path) {
            Ok(b) => {
                files.insert(name.clone(), Bytes::new(b));
            }
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        }
    }
    // fonts: the first installed of each list (Typst's embedded fonts as the last resort)
    let pick = |names: &[&str]| -> String {
        let book = fonts().book();
        let found: Vec<String> = names.iter().filter(|n| book.contains_family(&n.to_lowercase())).map(|n| format!("\"{n}\"")).collect();
        format!("({},)", found.join(", "))
    };
    source = source
        .replace("@@SANS@@", &pick(&["Inter", "Noto Sans", "DejaVu Sans", "Liberation Sans", "Libertinus Serif"]))
        .replace("@@MONO@@", &pick(&["JetBrains Mono", "JetBrainsMono NF", "DejaVu Sans Mono", "DejaVu Sans Mono", "Liberation Mono"]));
    let math_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let main = file_id("main.typ");
    let world = DocWorld { main, source: Source::new(main, source), files, library: LazyHash::new(Library::default()) };
    let t1 = Instant::now();
    let warned = typst::compile::<PagedDocument>(&world);
    let warnings: Vec<String> = warned.warnings.iter().map(|w| w.message.to_string()).collect();
    let doc = warned.output.map_err(|errs| errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    let compile_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let t2 = Instant::now();
    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()).map_err(|errs| errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    let pdf_ms = t2.elapsed().as_secs_f64() * 1000.0;
    let pages = doc.pages().iter().map(|p| preview_png(&typst_svg::svg(p, &typst_svg::SvgOptions::default()))).collect();
    Ok(Output { pdf, pages, warnings, ms: (math_ms, compile_ms, pdf_ms) })
}
