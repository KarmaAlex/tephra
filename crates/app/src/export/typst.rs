//! Note (Obsidian Markdown) → Typst markup, using the editor's own parser. Text is emitted as
//! Typst string literals (`#"…";`), so nothing in a note can be misread as Typst syntax.
//! Plugin-specific parts come through [`Ctx`]: numbers (Academic Notes), reference labels,
//! callout styles and bodies, code-block exports (charts, columns, the contents block).
//! Formulas become placeholders `@@M<n>@@` (see [`Emitter::math`]), filled in after MathJax has
//! typeset them; embedded files are listed in [`Emitter::files`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use tephra_core::markdown::tables::{parse_table, Align};
use tephra_core::markdown::{parse_blocks, parse_inline, BlockKind, Inline, InlineKind};

use crate::plugins::CalloutLook;

/// How a callout type looks, for export.
#[derive(Clone, Debug)]
pub struct Style {
    pub look: CalloutLook,
    /// (r, g, b).
    pub color: (u8, u8, u8),
    /// The type's name ("Theorem"); None = Obsidian's rule (title, else the capitalised type).
    pub label: Option<String>,
    /// The header icon (Obsidian look): an SVG drawn with `currentColor`.
    pub icon: Option<String>,
}

pub type Rgb = (u8, u8, u8);

/// The reading view's colours and sizes, for the PDF: the light variant of the theme's CSS
/// variables (paper is white) for the note's `cssclasses`. Sizes are in the reading view's
/// pixels (16 = the body text), which the PDF scales to its own text size (`u_`).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub text: Rgb,
    pub muted: Rgb,
    /// Checked task boxes, and the tick on them.
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub link: Rgb,
    /// Table lines, also the frame of code blocks; width 0 (`.NoBorder`) draws neither.
    pub border: Rgb,
    pub border_px: f32,
    pub code_bg: Rgb,
    pub inline_code_bg: Rgb,
    pub highlight: Rgb,
    pub table_head: Option<Rgb>,
    /// The header row's weight (Obsidian's `--table-header-weight`, semibold).
    pub table_head_weight: u16,
    pub quote: Rgb,
    pub quote_px: f32,
    pub hr: Rgb,
    pub hr_px: f32,
    /// H1–H6: size (em of the body), weight, colour (None = the text colour).
    pub headings: [(f32, u16, Option<Rgb>); 6],
    /// Display math centred (the editor's "Centre display math" setting), else indented.
    pub center_math: bool,
}

impl Default for Look {
    /// Obsidian's default light theme, as the editor shows it.
    fn default() -> Self {
        let muted = (0x5c, 0x5c, 0x5c);
        let accent = (138, 92, 245);
        Self {
            text: (0x22, 0x22, 0x22),
            muted,
            accent,
            on_accent: (255, 255, 255),
            link: accent,
            border: (0xe0, 0xe0, 0xe0),
            border_px: 1.0,
            code_bg: (0xfa, 0xfa, 0xfa),
            inline_code_bg: (236, 236, 236),
            highlight: (255, 236, 153),
            table_head: None,
            table_head_weight: 600,
            quote: accent,
            quote_px: 2.0,
            hr: (0xe0, 0xe0, 0xe0),
            hr_px: 2.0,
            headings: [(1.618, 700, None), (1.462, 600, None), (1.318, 600, None), (1.188, 600, None), (1.076, 600, None), (1.0, 600, Some(muted))],
            center_math: false,
        }
    }
}

/// Mixes `c` at opacity `a` onto white paper (the PDF draws no transparency).
pub fn on_white(c: Rgb, a: f32) -> Rgb {
    let m = |v: u8| (v as f32 * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    (m(c.0), m(c.1), m(c.2))
}

impl Look {
    /// The Typst definitions the helpers use (`u_` = one reading-view pixel, set by the caller).
    pub fn defs(&self) -> String {
        let c = |c: Rgb| rgb(c);
        let heads: Vec<String> = self
            .headings
            .iter()
            .enumerate()
            .map(|(i, (size, weight, color))| {
                // the editor's line heights: 1.25 for H1, 1.35 below
                let lh = if i == 0 { 1.25 } else { 1.35 };
                format!("({size:.3}, {weight}, {}, {lh})", color.map(c).unwrap_or("none".into()))
            })
            .collect();
        let border = if self.border_px > 0.0 { format!("{:.2} * u_ + {}", self.border_px, c(self.border)) } else { "none".into() };
        format!(
            "#let c_ = (text: {}, muted: {}, accent: {}, on-accent: {}, link: {}, code-bg: {}, inline-code: {}, highlight: {}, head: {}, quote: {}, hr: {})\n\
             #let b_ = {border}\n#let thw_ = {}\n#let qw_ = {:.2} * u_\n#let hrw_ = {:.2} * u_\n#let hs_ = ({},)\n#let center-math_ = {}\n",
            c(self.text),
            c(self.muted),
            c(self.accent),
            c(self.on_accent),
            c(self.link),
            c(self.code_bg),
            c(self.inline_code_bg),
            c(self.highlight),
            self.table_head.map(c).unwrap_or("none".into()),
            c(self.quote),
            c(self.hr),
            self.table_head_weight,
            self.quote_px.max(0.0),
            self.hr_px.max(0.5),
            heads.join(", "),
            self.center_math,
        )
    }
}

/// The virtual file holding [`SYNTAX_THEME`].
pub const SYNTAX_THEME_FILE: &str = "syntax_.tmTheme";

/// The editor's code colours (gpui-kit's "Default Light" syntax palette) as a TextMate theme,
/// for Typst's highlighter: keywords, strings, comments, numbers, functions, types, constants.
pub const SYNTAX_THEME: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>name</key><string>Tephra</string><key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#222222</string><key>background</key><string>#FAFAFA</string></dict></dict>
<dict><key>scope</key><string>comment, punctuation.definition.comment</string><key>settings</key><dict><key>foreground</key><string>#007FFF</string></dict></dict>
<dict><key>scope</key><string>string, punctuation.definition.string, constant.character.escape</string><key>settings</key><dict><key>foreground</key><string>#036A07</string></dict></dict>
<dict><key>scope</key><string>keyword, storage.modifier, storage.type.class, storage.type.struct, storage.type.function, storage.type.enum, keyword.operator.word</string><key>settings</key><dict><key>foreground</key><string>#0433FF</string></dict></dict>
<dict><key>scope</key><string>keyword.operator, punctuation</string><key>settings</key><dict><key>foreground</key><string>#222222</string></dict></dict>
<dict><key>scope</key><string>constant.numeric</string><key>settings</key><dict><key>foreground</key><string>#0433FF</string></dict></dict>
<dict><key>scope</key><string>constant.language, constant.other, support.constant</string><key>settings</key><dict><key>foreground</key><string>#C5060B</string></dict></dict>
<dict><key>scope</key><string>entity.name.function, support.function, meta.function-call entity.name</string><key>settings</key><dict><key>foreground</key><string>#0000A2</string></dict></dict>
<dict><key>scope</key><string>storage.type, support.type, entity.name.type, entity.name.class, entity.name.struct, entity.other.inherited-class</string><key>settings</key><dict><key>foreground</key><string>#6F42C1</string></dict></dict>
<dict><key>scope</key><string>meta.preprocessor, keyword.control.import, keyword.control.directive, entity.other.attribute-name, meta.annotation</string><key>settings</key><dict><key>foreground</key><string>#957931</string></dict></dict>
<dict><key>scope</key><string>entity.name.tag</string><key>settings</key><dict><key>foreground</key><string>#0433FF</string></dict></dict>
<dict><key>scope</key><string>variable, meta.property-name, support.variable</string><key>settings</key><dict><key>foreground</key><string>#333333</string></dict></dict>
</array></dict></plist>
"##;

/// Typst helpers that draw notes the way the editor's reading view does (gpui-kit's layout,
/// translated). Needs `u_`, `asc_`, `desc_` and [`Look::defs`]. Every text line is a fixed
/// line box (1.618 × the font size, glyphs centred, as GPUI lays out text) and blocks have no
/// spacing of their own: a blank line in the note is the space between blocks, as in the editor.
pub const HELPERS: &str = r##"
#let lh_(l) = (top-edge: (asc_ + (l - asc_ - desc_) / 2) * 1em, bottom-edge: -(desc_ + (l - asc_ - desc_) / 2) * 1em)
#set text(fill: c_.text, ..lh_(1.618))
#set par(leading: 0pt, spacing: 0pt, justify: false, first-line-indent: 0pt)
#set block(spacing: 0pt)
#set highlight(fill: c_.highlight, ..lh_(1.618))
#set raw(theme: "syntax_.tmTheme")
// Typst's own raw rule sets 0.8em: back to the size around it (inline code is text-sized)
#show raw: set text(font: @@MONO@@, size: 1.25em)
#show raw.where(block: false): it => box(fill: c_.inline-code, inset: (x: 2 * u_), outset: (y: 1 * u_), radius: 3 * u_, text(top-edge: "ascender", bottom-edge: "descender", it))
#show link: set text(fill: c_.link)
// Typst's own heading sizes and spacing are replaced (sizes are relative to the body text)
#show heading: set block(above: 0pt, below: 0pt)
#show heading: it => {
  let (size, weight, fill, l) = hs_.at(calc.min(it.level, 6) - 1)
  // kept on a page with what follows it
  block(width: 100%, sticky: true, {
    set text(size: size * body_, weight: weight, ..lh_(l))
    if fill == none { it.body } else { text(fill: fill, it.body) }
  })
}
#let blank_(n) = v(n * 1.618em)
#let ilink_(body) = text(fill: c_.link, body)
#let hr_() = block(width: 100%, inset: (y: 12 * u_), block(width: 100%, height: hrw_, fill: c_.hr))
#let li_(ind, mark, body) = pad(left: ind * u_, context {
  let m = text(fill: c_.muted, mark)
  let w = calc.max(22.4 * u_, measure(m).width + 6.72 * u_)
  grid(columns: (w, 1fr), align(right, pad(right: 6.72 * u_, m)), body)
})
#let tick_ = bytes("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='" + c_.on-accent.to-hex() + "' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M20 6 9 17l-5-5'/></svg>")
#let task_(ind, done, body) = pad(left: ind * u_, grid(
  columns: (15 * u_, 1fr),
  column-gutter: 8 * u_,
  pad(top: 5 * u_, if done {
    box(width: 15 * u_, height: 15 * u_, radius: 3 * u_, fill: c_.accent, stroke: 1 * u_ + c_.accent, align(center + horizon, image(tick_, format: "svg", width: 12 * u_)))
  } else {
    box(width: 15 * u_, height: 15 * u_, radius: 3 * u_, stroke: 1 * u_ + c_.muted)
  }),
  if done { text(fill: c_.muted, strike(body)) } else { body },
))
#let quote_(body) = block(width: 100%, stroke: (left: qw_ + c_.quote), inset: (left: 14.4 * u_ + qw_), text(fill: c_.muted, body))
#let code_(label, body) = block(
  width: 100%,
  fill: c_.code-bg,
  stroke: b_,
  inset: (x: 16 * u_, top: if label == "" { 14 * u_ } else { 28 * u_ }, bottom: 14 * u_),
  {
    if label != "" {
      place(top + right, dx: 4 * u_, dy: -21 * u_, text(font: @@SANS@@, size: 12 * u_, fill: c_.muted, ..lh_(16 / 12), label))
    }
    set text(size: 13 * u_, ..lh_(1.6))
    body
  },
)
#let dmath_(body, tag: none) = {
  let m = block(inset: (y: 4 * u_, left: if center-math_ { 0pt } else { 16 * u_ }), body)
  let m = if center-math_ { align(center, m) } else { m }
  if tag == none { block(width: 100%, m) } else {
    grid(columns: (1fr, auto), align: (left + horizon, right + horizon), m, pad(left: 16 * u_, tag))
  }
}
#let dmathi_(body) = {
  let m = box(inset: (y: 4 * u_, left: if center-math_ { 0pt } else { 16 * u_ }), body)
  if center-math_ { h(1fr) + m + h(1fr) } else { m }
}
#let img_(name, w) = box(layout(size => image(name, width: calc.min(w * u_, size.width))))
// Column widths from the content widths `nat` (padding included) in `room`, as gpui-kit lays
// tables out: cells grow alike up to their content's width, then a table narrower than the
// text spreads the rest in proportion to the columns; one wider shares the room equally among
// the columns that don't fit, none narrower than 64 px (or its content)
#let colw_(nat, room) = {
  let total = nat.sum()
  if total <= room { return nat.map(w => w / total * room) }
  let ws = nat.map(_ => 0pt)
  let open = range(nat.len())
  let left = room
  while open.len() > 0 {
    let share = left / open.len()
    let fit = open.filter(i => nat.at(i) <= share)
    if fit.len() == 0 {
      for i in open { ws.at(i) = calc.max(share, calc.min(nat.at(i), 64 * u_)) }
      open = ()
    } else {
      for i in fit { ws.at(i) = nat.at(i); left -= nat.at(i) }
      open = open.filter(i => i not in fit)
    }
  }
  ws
}
#let table_(aligns, ..cells) = layout(size => {
  let cells = cells.pos()
  let n = aligns.len()
  let rows = cells.chunks(n)
  // tables read a step smaller than the body (14/15), cells padded 12 × 7 px
  let ts = text.size * 14 / 15
  let cell(i, c) = text(size: ts, weight: if i < n { thw_ } else { "regular" }, c)
  let nat = range(n).map(i => calc.max(..rows.enumerate().map(((y, r)) => measure(cell(y * n + i, r.at(i))).width)) + 24 * u_)
  table(
    columns: colw_(nat, size.width),
    inset: (x: 12 * u_, y: 7 * u_),
    stroke: b_,
    fill: (x, y) => if y == 0 { c_.head } else { none },
    align: (x, y) => aligns.at(x) + top,
    ..cells.enumerate().map(((i, c)) => cell(i, c)),
  )
})
"##;

pub struct Ctx<'a> {
    /// Labels by line of the note: callout numbers ("Theorem 1.2"), equation numbers ("(1.2)").
    pub numbers: &'a HashMap<usize, String>,
    /// Display text of an internal link without alias (`target` as written, with `#sub`).
    pub link_label: &'a dyn Fn(&str) -> Option<String>,
    pub style: &'a dyn Fn(&str, &str) -> Style,
    /// A plugin's Typst for a code block (lang, code, nested-Markdown converter).
    pub code: &'a dyn Fn(&str, &str, &dyn Fn(&str) -> String) -> Option<String>,
    /// The note the editor shows above a code block it can't render ("Desmos plugin disabled").
    pub code_note: &'a dyn Fn(&str) -> Option<String>,
    /// A plugin's Typst for a callout body (kind, meta, body lines) and its end mark.
    pub callout_body: &'a dyn Fn(&str, &str, &[String], &dyn Fn(&str) -> String) -> Option<String>,
    pub body_end: &'a dyn Fn(&str) -> Option<String>,
    /// The file an embed (`![[name]]`) refers to.
    pub embed: &'a dyn Fn(&str) -> Option<PathBuf>,
}

pub struct Emitter<'a> {
    pub ctx: Ctx<'a>,
    /// Formulas to typeset: (TeX, display); placeholder `@@M<index>@@`.
    pub math: RefCell<Vec<(String, bool)>>,
    /// Files the document embeds: (virtual name, file on disk).
    pub files: RefCell<Vec<(String, PathBuf)>>,
}

/// A Typst string literal holding `s` as plain text.
pub fn lit(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut o = String::with_capacity(s.len() + 4);
    o.push_str("#\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o.push_str("\";");
    o
}

/// A Typst string expression (no `#`), for function arguments.
pub fn str_arg(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn rgb((r, g, b): (u8, u8, u8)) -> String {
    format!("rgb({r}, {g}, {b})")
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// White or near-black, whichever reads better on a filled badge of colour `c`.
fn on_color((r, g, b): (u8, u8, u8)) -> &'static str {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let l = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    if 1.05 / (l + 0.05) >= (l + 0.05) / 0.0603 { "white" } else { "rgb(26, 26, 26)" }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn line_of(text: &str, off: usize) -> usize {
    text[..off.min(text.len())].matches('\n').count()
}

/// Removes one level of quote markers (`>` and one space) from every line; the line count stays.
fn dequote(s: &str) -> String {
    s.split('\n')
        .map(|l| {
            let t = l.trim_start();
            match t.strip_prefix('>') {
                Some(r) => r.strip_prefix(' ').unwrap_or(r),
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl<'a> Emitter<'a> {
    pub fn new(ctx: Ctx<'a>) -> Self {
        Self { ctx, math: RefCell::default(), files: RefCell::default() }
    }

    fn math(&self, tex: &str, display: bool) -> String {
        let mut m = self.math.borrow_mut();
        m.push((tex.trim().to_string(), display));
        format!("@@M{}@@", m.len() - 1)
    }

    fn file(&self, p: PathBuf) -> String {
        let mut files = self.files.borrow_mut();
        if let Some((n, _)) = files.iter().find(|(_, q)| *q == p) {
            return n.clone();
        }
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = format!("f{}.{ext}", files.len());
        files.push((name.clone(), p));
        name
    }

    /// An image block/inline: as wide as the editor shows it (its pixels, in reading-view
    /// pixels), at most the width it stands in.
    fn image(&self, p: PathBuf) -> String {
        let w = image::image_dimensions(&p).ok().map(|(w, _)| w);
        let name = self.file(p);
        match w {
            Some(w) => format!("#img_({}, {w});", str_arg(&name)),
            None => format!("#box(image({}, width: 100%));", str_arg(&name)),
        }
    }

    /// Inline content of `range`.
    fn inline(&self, text: &str, range: Range<usize>) -> String {
        let nodes = parse_inline(text, range.clone());
        self.inlines(text, &nodes, range)
    }

    fn plain(&self, s: &str) -> String {
        // HTML that notes use: <br> breaks the line, other tags are dropped
        let br = regex::Regex::new(r"(?i)<br\s*/?>").expect("regex");
        let tags = regex::Regex::new(r"(?i)</?(u|sup|sub|font|span|center|p|mark|b|i|em|strong)(\s[^>]*)?>").expect("regex");
        let mut out = String::new();
        for (i, part) in br.split(s).enumerate() {
            if i > 0 {
                out.push_str("#linebreak();");
            }
            out.push_str(&lit(&tags.replace_all(part, "")));
        }
        out
    }

    fn inlines(&self, text: &str, nodes: &[Inline], range: Range<usize>) -> String {
        let mut out = String::new();
        let mut pos = range.start;
        for n in nodes {
            if n.range.start > pos {
                out += &self.plain(&text[pos..n.range.start]);
            }
            out += &self.node(text, n);
            pos = n.range.end;
        }
        if pos < range.end {
            out += &self.plain(&text[pos..range.end]);
        }
        out
    }

    fn node(&self, text: &str, n: &Inline) -> String {
        let inner = |me: &Self| me.inlines(text, &n.children, n.inner.clone());
        match n.kind {
            InlineKind::Strong => format!("#strong[{}];", inner(self)),
            InlineKind::Emph => format!("#emph[{}];", inner(self)),
            InlineKind::Strike => format!("#strike[{}];", inner(self)),
            InlineKind::Highlight => format!("#highlight[{}];", inner(self)),
            InlineKind::Code => format!("#raw({});", str_arg(&text[n.inner.clone()])),
            // display math within a line of text: the editor's inline block (indented, or
            // centred in its line), a little air above and below
            InlineKind::Math { display } => {
                let m = self.math(&text[n.inner.clone()], display);
                let m = if display { format!("#dmathi_[{m}];") } else { m };
                // a formula touching a word wraps with it, as in the editor ("$x$," never
                // leaves the comma at the start of a line)
                let glued = |c: Option<char>| c.is_some_and(|c| !c.is_whitespace());
                let wj = "#sym.wj;";
                format!(
                    "{}{m}{}",
                    if glued(text[..n.range.start].chars().next_back()) { wj } else { "" },
                    if glued(text[n.range.end..].chars().next()) { wj } else { "" }
                )
            }
            InlineKind::Escape => lit(&text[n.inner.clone()]),
            InlineKind::Comment | InlineKind::BlockId => String::new(),
            InlineKind::Tag => format!("#ilink_[{}];", lit(&text[n.range.clone()])),
            InlineKind::WikiLink { embed: true } => {
                let target = n.link_target(text);
                match (self.ctx.embed)(target) {
                    Some(p) if crate::embeds::is_image(&p) => self.image(p),
                    _ => format!("#ilink_[{}];", lit(target)),
                }
            }
            InlineKind::WikiLink { embed: false } => {
                let target = n.link_target(text);
                let raw = &text[n.range.start + 2..n.range.end - 2];
                let shown = if raw.contains('|') {
                    text[n.inner.clone()].to_string()
                } else {
                    (self.ctx.link_label)(target).unwrap_or_else(|| {
                        let (note, sub) = target.split_once('#').unwrap_or((target, ""));
                        let note = note.rsplit('/').next().unwrap_or(note);
                        match (note.is_empty(), sub.is_empty()) {
                            (_, true) => note.to_string(),
                            (true, false) => sub.trim_start_matches('^').to_string(),
                            (false, false) => format!("{note} > {}", sub.trim_start_matches('^')),
                        }
                    })
                };
                format!("#ilink_[{}];", lit(&shown))
            }
            InlineKind::Link => {
                let url = n.link_target(text).to_string();
                let label = inner(self);
                if url.contains("://") || url.starts_with("mailto:") {
                    format!("#link({})[{}];", str_arg(&url), label)
                } else {
                    format!("#ilink_[{label}];")
                }
            }
            InlineKind::Html => {
                use tephra_core::markdown::inline::{html_attr, html_tag_name};
                let open = &text[n.range.start..n.inner.start];
                let style = html_attr(open, "style").unwrap_or_default();
                let col = |v: Option<String>| v.and_then(|c| crate::css::parse_color(&c)).map(|h| {
                    let c = gpui_kit::Rgba::from(h);
                    format!("rgb({}, {}, {})", (c.r * 255.).round(), (c.g * 255.).round(), (c.b * 255.).round())
                });
                let mut body = inner(self);
                body = match html_tag_name(open).as_str() {
                    "u" | "ins" => format!("#underline[{body}];"),
                    "sup" => format!("#super[{body}];"),
                    "sub" => format!("#sub[{body}];"),
                    "b" | "strong" => format!("#strong[{body}];"),
                    "i" | "em" => format!("#emph[{body}];"),
                    "s" | "del" | "strike" => format!("#strike[{body}];"),
                    "mark" => format!("#highlight[{body}];"),
                    _ => body,
                };
                let fg = col(crate::css::style_prop(&style, "color")).or_else(|| col(html_attr(open, "color")));
                if let Some(c) = fg {
                    body = format!("#text(fill: {c})[{body}];");
                }
                if let Some(c) = col(crate::css::style_prop(&style, "background").or_else(|| crate::css::style_prop(&style, "background-color"))) {
                    body = format!("#highlight(fill: {c})[{body}];");
                }
                let align = if html_tag_name(open) == "center" { Some("center".to_string()) } else { html_attr(open, "align").or_else(|| crate::css::style_prop(&style, "text-align")) };
                match align.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("center") => format!("#align(center)[{body}];"),
                    Some("right") => format!("#align(right)[{body}];"),
                    Some("justify") => format!("#par(justify: true)[{body}];"),
                    _ => body,
                }
            }
            InlineKind::Image => {
                let url = n.link_target(text);
                match (!url.contains("://")).then(|| (self.ctx.embed)(&url.replace("%20", " "))).flatten() {
                    Some(p) => self.image(p),
                    None => lit(&text[n.inner.clone()]),
                }
            }
        }
    }

    /// Inline content where each source line break is a line break (Obsidian's default).
    fn lines_inline(&self, text: &str, range: Range<usize>) -> String {
        let mut parts = Vec::new();
        let mut s = range.start;
        for (i, l) in text[range.clone()].split('\n').enumerate() {
            let e = s + l.len();
            let r = s..e - usize::from(l.ends_with('\r'));
            let _ = i;
            parts.push(self.inline(text, r));
            s = e + 1;
        }
        parts.join("#linebreak();\n")
    }

    /// Converts a whole note body (or nested Markdown); `line_base` = line of `text` in the note.
    pub fn blocks(&self, text: &str, line_base: usize) -> String {
        self.blocks_sep(text, line_base, None)
    }

    /// Like [`Self::blocks`], with `sep` (Typst) around tables, display math and quotes: a
    /// callout's body has a gap between its children, and the editor makes one child of each
    /// run of text lines (lists included), and one of each table, formula or nested callout.
    fn blocks_sep(&self, text: &str, line_base: usize, sep: Option<&str>) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut boxed: Vec<bool> = Vec::new();
        // blank lines: as tall as a line of text, as in the editor (blocks have no margins)
        let mut blank = 0;
        for b in parse_blocks(text) {
            let line = line_base + line_of(text, b.range.start);
            let indent = b.indent as f32 * 6.0; // the editor's list nesting (px per column)
            let piece = match &b.kind {
                BlockKind::Blank => {
                    blank += 1;
                    continue;
                }
                BlockKind::Frontmatter => continue,
                BlockKind::Heading { level } => format!("#heading(level: {level})[{}]", self.inline(text, b.content.clone())),
                BlockKind::Paragraph => self.lines_inline(text, b.range.clone()),
                BlockKind::ListItem { task: Some(done), .. } => format!("#task_({indent}, {done})[{}]", self.lines_inline(text, b.content.clone())),
                BlockKind::ListItem { ordered, .. } => {
                    let marker = if *ordered {
                        let n = text[b.marker.clone()].trim().trim_end_matches(['.', ')']).trim().parse::<u32>().unwrap_or(1);
                        format!("{n}.")
                    } else {
                        "•".into()
                    };
                    format!("#li_({indent}, {})[{}]", str_arg(&marker), self.lines_inline(text, b.content.clone()))
                }
                BlockKind::Rule => "#hr_()".into(),
                BlockKind::MathBlock => {
                    let m = self.math(&text[b.content.clone()], true);
                    match self.ctx.numbers.get(&line) {
                        Some(tag) => format!("#dmath_([{m}], tag: [{}])", lit(tag)),
                        None => format!("#dmath_([{m}])"),
                    }
                }
                BlockKind::Code { lang, .. } => {
                    let code = &text[b.content.clone()];
                    // nested Markdown in a plugin block (columns…) goes through this emitter too
                    let nested = |md: &str| self.blocks(md, 0);
                    match (self.ctx.code)(lang, code, &nested) {
                        Some(t) => t,
                        None => {
                            // the label the editor shows (its highlighter's name for the language)
                            let label = crate::editor::projection::code_lang(lang).unwrap_or_default();
                            let block = format!("#code_({}, raw(block: true, lang: {}, {}))", str_arg(&label), str_arg(&lang.to_lowercase()), str_arg(code));
                            match (self.ctx.code_note)(lang) {
                                Some(note) => format!("#text(size: 12 * u_, fill: c_.muted, ..lh_(16 / 12))[{}]\n\n{block}", lit(&note)),
                                None => block,
                            }
                        }
                    }
                }
                BlockKind::Table => self.table(text, b.range.clone()),
                BlockKind::Quote => self.quote(&text[b.range.clone()], line),
            };
            if blank > 0 {
                out.push(format!("#blank_({blank})"));
                boxed.push(false);
                blank = 0;
            }
            out.push(piece);
            boxed.push(matches!(b.kind, BlockKind::Table | BlockKind::MathBlock | BlockKind::Quote));
        }
        // trailing blank lines are dropped: nothing follows them
        let mut joined = String::new();
        for (i, p) in out.iter().enumerate() {
            if i > 0 {
                joined.push_str("\n\n");
                if let Some(s) = sep.filter(|_| boxed[i] || boxed[i - 1]) {
                    joined.push_str(s);
                    joined.push_str("\n\n");
                }
            }
            joined.push_str(p);
        }
        joined
    }

    fn table(&self, text: &str, r: Range<usize>) -> String {
        let Some(t) = parse_table(text, r.clone()) else { return self.lines_inline(text, r) };
        let cols = t.rows[0].len().max(1);
        let aligns: Vec<&str> = (0..cols)
            .map(|i| match t.aligns.get(i) {
                Some(Align::Center) => "center",
                Some(Align::Right) => "right",
                _ => "left",
            })
            .collect();
        let mut cells = Vec::new();
        for row in &t.rows {
            for c in 0..cols {
                cells.push(format!("[{}]", row.get(c).map(|cell| self.inline(text, cell.clone())).unwrap_or_default()));
            }
        }
        format!("#table_(({},), {})", aligns.join(", "), cells.join(", "))
    }

    /// A blockquote or callout (`src` = its lines, `line` = its first line in the note), drawn
    /// as `editor::render` draws it (sizes in reading-view pixels, `u_`).
    fn quote(&self, src: &str, line: usize) -> String {
        let inner = dequote(src);
        let first = inner.lines().next().unwrap_or("");
        let header = regex::Regex::new(r"^\[!([^\]|]+)(?:\|([^\]]*))?\]([+-]?)\s*(.*)$").expect("regex");
        let Some(c) = header.captures(first.trim_start()) else {
            return format!("#quote_[\n{}\n]", self.blocks(&inner, line));
        };
        let kind = c[1].trim().to_lowercase();
        let meta = c.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        let title_src = c.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();
        let body = inner.split_once('\n').map(|(_, b)| b.to_string()).unwrap_or_default();
        let style = (self.ctx.style)(&kind, &meta);
        let number = self.ctx.numbers.get(&line).cloned();
        let title = {
            let t = title_src.clone();
            self.inline(&t, 0..t.len())
        };
        let body_lines: Vec<String> = body.lines().map(String::from).collect();
        let nested = |m: &str| self.inline(m, 0..m.len());
        // the body's children have a 4 px gap between them (and after the header)
        const GAP: &str = "#v(4 * u_)";
        let mut body_t = match (self.ctx.callout_body)(&kind, &meta, &body_lines, &nested) {
            Some(t) => t,
            None => self.blocks_sep(&body, line + 1, Some(GAP)),
        };
        let col = rgb(style.color);
        // the end mark (proof □): its own row, at the right, in the callout's colour
        if let Some(m) = (self.ctx.body_end)(&kind) {
            body_t = format!("{body_t}\n\n{GAP}\n\n#align(right, text(fill: {col})[{}])", lit(&m));
        }
        let body_t = if body_t.trim().is_empty() { String::new() } else { format!("{GAP}\n\n{body_t}") };
        let label = number.or_else(|| style.label.clone()).unwrap_or_default();
        let tinted = |a: f32| rgb(on_white(style.color, a));
        match style.look {
            CalloutLook::Obsidian => {
                // icon and title in the colour, semibold; no title: the capitalised type
                let head = if title_src.trim().is_empty() { lit(&capitalize(&kind)) } else { title };
                let head = format!("text(fill: {col}, weight: 600)[{head}]");
                let row = match &style.icon {
                    Some(svg) => format!(
                        "#block(sticky: true, grid(columns: (16 * u_, 1fr), column-gutter: 8 * u_, align: horizon, image(bytes({}), format: \"svg\", width: 16 * u_), {head}))",
                        str_arg(&svg.replace("currentColor", &hex(style.color)))
                    ),
                    None => format!("#block(sticky: true, {head})"),
                };
                format!("#v(4 * u_)\n#block(width: 100%, inset: (left: 24 * u_, right: 12 * u_, y: 12 * u_), radius: 4 * u_, fill: {})[\n{row}\n\n{body_t}\n]\n#v(4 * u_)", tinted(0.1))
            }
            CalloutLook::Badge => format!(
                "#v(4 * u_)\n#block(width: 100%, inset: (x: 12 * u_, y: 8 * u_), radius: 6 * u_, stroke: 1.4 * u_ + {col}, fill: {})[\n#block(sticky: true, grid(columns: (auto, 1fr), column-gutter: 8 * u_, align: horizon, box(fill: {col}, inset: (x: 8 * u_, y: 2 * u_), radius: 4 * u_, text(size: 14 * u_, weight: 600, fill: {on})[{}]), [{title}]))\n\n{body_t}\n]\n#v(4 * u_)",
                tinted(0.06),
                lit(&label),
                on = on_color(style.color)
            ),
            CalloutLook::Soft => format!(
                "#v(4 * u_)\n#block(width: 100%, inset: (left: 12 * u_ + 2 * u_), stroke: (left: 2 * u_ + {}))[\n#block(sticky: true)[#text(style: \"italic\", weight: 600, fill: {col})[{}]#h(8 * u_)#emph[{title}]]\n\n{body_t}\n]\n#v(4 * u_)",
                tinted(0.6),
                lit(&if label.is_empty() { capitalize(&kind) } else { label.clone() })
            ),
            CalloutLook::Rules => format!(
                "#v(4 * u_)\n#block(width: 100%, inset: (x: 8 * u_, y: 4 * u_), stroke: (top: 2 * u_ + {col}, bottom: 2 * u_ + {col}))[\n#block(sticky: true)[#text(weight: 700, fill: {col})[{}]#h(8 * u_){title}]\n\n{body_t}\n]\n#v(4 * u_)",
                lit(&if label.is_empty() { capitalize(&kind) } else { label.clone() })
            ),
            CalloutLook::Caption { below } => {
                let cap = if label.is_empty() { format!("#align(center)[{title}]") } else { format!("#align(center)[#strong[{}]#h(8 * u_){title}]", lit(&label)) };
                let body_c = if body_t.is_empty() { String::new() } else { format!("#align(center)[\n{body_t}\n]") };
                let (a, b) = if below { (body_c, cap) } else { (cap, body_c) };
                format!("#v(8 * u_)\n#block(width: 100%)[\n{a}\n\n{b}\n]\n#v(8 * u_)")
            }
        }
    }
}

/// Typst for a MathJax SVG (as the M0 spike did): inline on the baseline; display math as a
/// box for `dmath_`.
pub fn math_snippet(name: &str, svg: &str, display: bool) -> String {
    const EX_EM: f32 = 0.442; // MathJax ex in em (Computer Modern x-height)
    let attr = |n: &str| -> Option<f32> {
        let pat = format!(" {n}=\"");
        let s = svg.find(&pat)? + pat.len();
        svg[s..].split('"').next()?.trim_end_matches("ex").parse().ok()
    };
    let h = attr("height").unwrap_or(2.0);
    let va = svg
        .split("vertical-align:")
        .nth(1)
        .and_then(|v| v.trim().split("ex").next())
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    if display {
        // the editor draws display math 1.1× the text size, on the baseline when it stands in
        // a line of text; `dmath_`/`dmathi_` place it
        return format!("#box(baseline: {:.3}em, image({}, height: {:.3}em));", -va * EX_EM * 1.1, str_arg(name), h * EX_EM * 1.1);
    }
    let img = format!("image({}, height: {:.3}em)", str_arg(name), h * EX_EM);
    {
        format!("#box(baseline: {:.3}em, {img});", -va * EX_EM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_test(f: impl FnOnce(&Emitter)) {
        let numbers: HashMap<usize, String> = [(2, "Theorem 1.1".to_string()), (6, "(1.1)".to_string())].into_iter().collect();
        let ctx = Ctx {
            numbers: &numbers,
            link_label: &|t: &str| (t == "#^e1").then(|| "eq:1.1".to_string()),
            style: &|k: &str, _: &str| Style { look: if k == "thm" { CalloutLook::Badge } else { CalloutLook::Obsidian }, color: (40, 100, 110), label: (k == "thm").then(|| "Theorem".into()), icon: (k == "note").then(|| "<svg stroke=\"currentColor\"/>".into()) },
            code: &|lang: &str, _: &str, _: &dyn Fn(&str) -> String| (lang == "chart").then(|| "#box[CHART]".to_string()),
            code_note: &|lang: &str| (lang == "desmos-graph").then(|| "Desmos plugin disabled".to_string()),
            callout_body: &|_, _, _, _| None,
            body_end: &|k: &str| (k == "pf").then(|| "□".to_string()),
            embed: &|_| None,
        };
        let e = Emitter::new(ctx);
        f(&e);
    }

    #[test]
    fn literals_cannot_inject_typst() {
        assert_eq!(lit("a \"b\" \\ #c"), "#\"a \\\"b\\\" \\\\ #c\";");
        assert_eq!(lit(""), "");
    }

    #[test]
    fn converts_the_main_constructs() {
        ctx_test(|e| {
            let t = "# Title\n## Sec\n> [!thm] Big\n> body $x^2$\n\nsee [[#^e1]] and **bold** ==hi==\n$$ a=b $$\n^e1\n- one\n\t- [x] two\n3. three\n\n```chart\ntype: bar\n```\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n> [!pf]\n> done\n\n> [!note]\n> text\n> | x |\n> |---|\n> | y |\n\n```C++\nint x;\n```\n---\n> quoted\n\n";
            let out = e.blocks(t, 0);
            assert!(out.contains("#heading(level: 1)[#\"Title\";]"), "{out}");
            assert!(out.contains("#heading(level: 1)[#\"Title\";]\n\n#heading(level: 2)"), "no gap where the note has no blank line");
            assert!(out.contains("text(size: 14 * u_, weight: 600, fill: white)[#\"Theorem 1.1\";]") && out.contains("#\"Big\";"), "numbered badge: {out}");
            assert!(out.contains("@@M0@@"), "inline math placeholder");
            assert!(out.contains("#\"eq:1.1\";"), "reference label");
            assert!(out.contains("#strong[#\"bold\";];") && out.contains("#highlight["));
            assert!(out.contains("#dmath_([@@M1@@], tag: [#\"(1.1)\";])"), "numbered equation");
            assert!(out.contains("#li_(0, \"•\")[#\"one\";]\n\n#task_(24, true)[#\"two\";]\n\n#li_(0, \"3.\")[#\"three\";]"), "list items, the task nested: {out}");
            assert!(out.contains("#box[CHART]"));
            assert!(out.contains("#table_((left, right,), [#\"a\";], [#\"b\";], [#\"1\";], [#\"2\";])"), "cells as written (the helper makes the header semibold)");
            assert!(out.contains("#align(right, text(fill: rgb(40, 100, 110))[#\"□\";])"), "proof end mark on its own row");
            assert!(out.contains("#blank_(1)"), "blank lines are space");
            assert!(!out.trim_end().ends_with("#blank_(1)"), "trailing blank lines are dropped");
            // Obsidian look: tinted box, icon in the callout's colour, gaps only around the table
            assert!(out.contains("fill: rgb(234, 240, 241)") && out.contains("<svg stroke=\\\"#28646e\\\"/>"), "{out}");
            assert!(out.contains("#\"text\";\n\n#v(4 * u_)\n\n#table_("), "gap before the table in a callout: {out}");
            assert!(out.contains("#code_(\"cpp\", raw(block: true, lang: \"c++\", \"int x;\"))"), "label as the editor shows it");
            assert!(out.contains("#hr_()") && out.contains("#quote_["));
            let d = e.blocks("```desmos-graph\nx\n```", 0);
            assert!(d.starts_with("#text(size: 12 * u_, fill: c_.muted, ..lh_(16 / 12))[#\"Desmos plugin disabled\";]\n\n#code_("), "{d}");
            assert_eq!(e.math.borrow().iter().map(|m| m.1).collect::<Vec<_>>(), vec![false, true]);
        });
    }

    #[test]
    fn display_math_inside_a_line_is_placed_like_the_editor() {
        ctx_test(|e| {
            let out = e.blocks("text $$x$$ more", 0);
            assert!(out.contains("#dmathi_[@@M0@@];"), "{out}");
            let out = e.blocks("a $x$, b", 0);
            assert!(out.contains("@@M1@@#sym.wj;#\", b\";"), "the comma stays with the formula: {out}");
        });
    }

    #[test]
    fn look_defs_follow_the_theme() {
        let mut l = Look::default();
        assert!(l.defs().contains("#let b_ = 1.00 * u_ + rgb(224, 224, 224)") && l.defs().contains("#let thw_ = 600"));
        l.border_px = 0.0; // .NoBorder
        l.headings[0] = (2.0, 800, Some((200, 0, 0)));
        let d = l.defs();
        assert!(d.contains("#let b_ = none"), "{d}");
        assert!(d.contains("(2.000, 800, rgb(200, 0, 0), 1.25)"), "{d}");
        assert_eq!(on_white((0, 0, 0), 0.075), (236, 236, 236), "translucent colours on paper");
    }

    #[test]
    fn math_snippets_follow_mathjax_metrics() {
        let svg = r#"<svg style="vertical-align: -0.566ex;" width="5.2ex" height="2.262ex"></svg>"#;
        assert_eq!(math_snippet("m0.svg", svg, false), "#box(baseline: 0.250em, image(\"m0.svg\", height: 1.000em));");
        assert_eq!(math_snippet("m0.svg", svg, true), "#box(baseline: 0.275em, image(\"m0.svg\", height: 1.100em));", "1.1× the text, as the editor");
    }
}
