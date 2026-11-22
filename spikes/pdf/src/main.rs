//! PDF spike: compile a Typst document (generated from a note) to PDF in-process.
//! usage: spike-pdf <dir-with-main.typ> <out.pdf>
//! Files referenced by main.typ are read relative to <dir> (symlink Images/ there).
use std::path::{Path, PathBuf};
use std::time::Instant;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst_layout::PagedDocument;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;

struct SpikeWorld {
    root: PathBuf,
    main: FileId,
    library: LazyHash<Library>,
    fonts: FontStore,
}

impl SpikeWorld {
    fn path(&self, id: FileId) -> FileResult<PathBuf> {
        let rel = id.get().vpath().get_without_slash().to_string();
        let p = self.root.join(rel);
        if p.exists() { Ok(p) } else { Err(FileError::NotFound(p)) }
    }
}

impl World for SpikeWorld {
    fn library(&self) -> &LazyHash<Library> { &self.library }
    fn book(&self) -> &LazyHash<FontBook> { self.fonts.book() }
    fn main(&self) -> FileId { self.main }
    fn source(&self, id: FileId) -> FileResult<Source> {
        let p = self.path(id)?;
        let text = std::fs::read_to_string(&p).map_err(|e| FileError::from_io(e, &p))?;
        Ok(Source::new(id, text))
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let p = self.path(id)?;
        std::fs::read(&p).map(Bytes::new).map_err(|e| FileError::from_io(e, &p))
    }
    fn font(&self, index: usize) -> Option<Font> { self.fonts.font(index) }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let now = chrono::Local::now().date_naive();
        use chrono::Datelike as _;
        Datetime::from_ymd(now.year(), now.month() as u8, now.day() as u8)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let out = Path::new(&args[2]);
    let t0 = Instant::now();
    let mut fonts = FontStore::new();
    fonts.extend(typst_kit::fonts::embedded());
    fonts.extend(typst_kit::fonts::system());
    let fonts_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let main = FileId::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new("main.typ").expect("vpath")));
    let world = SpikeWorld { root, main, library: LazyHash::new(Library::default()), fonts };
    let t1 = Instant::now();
    let warned = typst::compile::<PagedDocument>(&world);
    for w in &warned.warnings { eprintln!("warning: {}", w.message); }
    let doc = match warned.output {
        Ok(d) => d,
        Err(errs) => { for e in errs { eprintln!("error: {} {:?}", e.message, e.span); } std::process::exit(1); }
    };
    let compile_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let t2 = Instant::now();
    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()).expect("pdf export");
    let pdf_ms = t2.elapsed().as_secs_f64() * 1000.0;
    std::fs::write(out, &pdf).expect("write pdf");
    println!("PDFSPIKE pages={} fonts_ms={fonts_ms:.0} compile_ms={compile_ms:.0} pdf_ms={pdf_ms:.0} bytes={}", doc.pages().len(), pdf.len());
}
