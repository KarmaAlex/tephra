//! Asset source: gpui-kit's default icons plus the extra Lucide icons we use (Obsidian also uses
//! Lucide, so callout icons match).

use std::borrow::Cow;

use gpui_kit::*;

gpui_kit::assets::icon_assets!(
    pub ExtraIcons,
    [Pencil, ClipboardList, Info, CircleCheck, Flame, Check, CircleQuestionMark, TriangleAlert, X, Zap, Bug, List, Quote, Puzzle, Settings, Sigma, Folder, PanelRight, Star, Hash, ListOrdered, Bookmark, Link, ArrowUpRight, ArrowDownLeft, Tags, Heading, ListTree, Image, TextSearch, Unlink, Undo2, Redo2, Copy, Scissors, ClipboardPaste, Eraser, Heading1, Heading2, Heading3, Heading4, Heading5, Heading6, Bold, Italic, Strikethrough, Underline, Highlighter, Superscript, Subscript, Code, SquareCode, SquareSigma, Paperclip, Table, ListChecks, TextQuote, MessageSquare, FileText, Minus, ListOrdered, ListIndentDecrease, ListIndentIncrease, TextAlignJustify, TextAlignStart, TextAlignCenter, TextAlignEnd, Baseline, PaintBucket, Maximize, ArrowDown, ArrowUp, Box, Columns2, FileDown, ChartColumn, Printer, Calendar, CalendarDays, Files, FileClock, GitFork, Combine, Palette, Clock, Focus, GripVertical, SeparatorVertical, RotateCcw, ListPlus, FolderInput, ArrowUpNarrowWide]
);

pub struct AppAssets;

/// Images bundled with the app (the logo on the welcome page), served from `assets/`.
pub const LOGO: &str = "images/tephra-logo.png";

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(&include_bytes!("../assets/tephra-logo.png")[..])));
        }
        if let Some(b) = ExtraIcons.load(path)? {
            return Ok(Some(b));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_kit::assets::Assets.list(path)?;
        v.extend(ExtraIcons.list(path)?);
        if path.is_empty() || path == "images" {
            v.push(SharedString::from(LOGO));
        }
        v.sort();
        v.dedup();
        Ok(v)
    }
}
