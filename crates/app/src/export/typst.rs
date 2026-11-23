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

use obsidilight_core::markdown::tables::{parse_table, Align};
use obsidilight_core::markdown::{parse_blocks, parse_inline, BlockKind, Inline, InlineKind};

use crate::plugins::CalloutLook;

/// How a callout type looks, for export.
#[derive(Clone, Debug)]
pub struct Style {
    pub look: CalloutLook,
    /// (r, g, b).
    pub color: (u8, u8, u8),
    /// The type's name ("Theorem"); None = Obsidian's rule (title, else the capitalised type).
    pub label: Option<String>,
}

pub struct Ctx<'a> {
    /// Labels by line of the note: callout numbers ("Theorem 1.2"), equation numbers ("(1.2)").
    pub numbers: &'a HashMap<usize, String>,
    /// Display text of an internal link without alias (`target` as written, with `#sub`).
    pub link_label: &'a dyn Fn(&str) -> Option<String>,
    pub style: &'a dyn Fn(&str, &str) -> Style,
    /// A plugin's Typst for a code block (lang, code, nested-Markdown converter).
    pub code: &'a dyn Fn(&str, &str, &dyn Fn(&str) -> String) -> Option<String>,
    /// A plugin's Typst for a callout body (kind, meta, body lines) and its end mark.
    pub callout_body: &'a dyn Fn(&str, &str, &[String], &dyn Fn(&str) -> String) -> Option<String>,
    pub body_end: &'a dyn Fn(&str) -> Option<String>,
    /// The file an embed (`![[name]]`) refers to.
    pub embed: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Tables without borders (the note has `cssclasses: NoBorder`).
    pub no_border: bool,
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

    /// An image block/inline: as wide as it is (1 px = 0.75 pt), at most the text width.
    fn image(&self, p: PathBuf) -> String {
        let w = image::image_dimensions(&p).ok().map(|(w, _)| w as f32 * 0.75);
        let name = self.file(p);
        match w {
            Some(w) if w < 520.0 => format!("#box(image({}, width: {w:.0}pt));", str_arg(&name)),
            _ => format!("#box(image({}, width: 100%));", str_arg(&name)),
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
            InlineKind::Highlight => format!("#highlight(fill: rgb(255, 232, 124))[{}];", inner(self)),
            InlineKind::Code => format!("#raw({});", str_arg(&text[n.inner.clone()])),
            InlineKind::Math { display } => self.math(&text[n.inner.clone()], display),
            InlineKind::Escape => lit(&text[n.inner.clone()]),
            InlineKind::Comment | InlineKind::BlockId => String::new(),
            InlineKind::Tag => format!("#text(fill: rgb(80, 110, 200))[{}];", lit(&text[n.range.clone()])),
            InlineKind::WikiLink { embed: true } => {
                let target = n.link_target(text);
                match (self.ctx.embed)(target) {
                    Some(p) if crate::embeds::is_image(&p) => self.image(p),
                    _ => format!("#text(fill: rgb(120, 90, 200))[{}];", lit(target)),
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
                format!("#text(fill: rgb(120, 90, 200))[{}];", lit(&shown))
            }
            InlineKind::Link => {
                let url = n.link_target(text).to_string();
                let label = inner(self);
                if url.contains("://") || url.starts_with("mailto:") {
                    format!("#link({})[{}];", str_arg(&url), label)
                } else {
                    format!("#text(fill: rgb(120, 90, 200))[{label}];")
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
        let mut out: Vec<String> = Vec::new();
        let mut prev_list = false;
        for b in parse_blocks(text) {
            let line = line_base + line_of(text, b.range.start);
            let is_list = matches!(b.kind, BlockKind::ListItem { .. });
            let piece = match &b.kind {
                BlockKind::Blank | BlockKind::Frontmatter => None,
                BlockKind::Heading { level } => Some(format!("{} {}", "=".repeat(*level as usize), self.inline(text, b.content.clone()))),
                BlockKind::Paragraph => Some(self.lines_inline(text, b.range.clone())),
                BlockKind::ListItem { ordered, task } => {
                    let depth = b.indent / 2;
                    let marker = if *ordered {
                        let n = text[b.marker.clone()].trim().trim_end_matches(['.', ')']).trim().parse::<u32>().unwrap_or(1);
                        format!("{n}. ")
                    } else {
                        "- ".into()
                    };
                    let tick = match task {
                        Some(true) => "☑ ",
                        Some(false) => "☐ ",
                        None => "",
                    };
                    Some(format!("{}{marker}{tick}{}", "  ".repeat(depth), self.lines_inline(text, b.content.clone())))
                }
                BlockKind::Rule => Some("#line(length: 100%, stroke: 0.5pt + gray)".into()),
                BlockKind::MathBlock => {
                    let m = self.math(&text[b.content.clone()], true);
                    Some(match self.ctx.numbers.get(&line) {
                        Some(tag) => format!("#grid(columns: (1fr, auto), align: (center + horizon, right + horizon), [{m}], [{}])", lit(tag)),
                        None => format!("#align(center)[{m}]"),
                    })
                }
                BlockKind::Code { lang, .. } => {
                    let code = &text[b.content.clone()];
                    // nested Markdown in a plugin block (columns…) goes through this emitter too
                    let nested = |md: &str| self.blocks(md, 0);
                    match (self.ctx.code)(lang, code, &nested) {
                        Some(t) => Some(t),
                        None => Some(format!(
                            "#block(width: 100%, fill: luma(245), inset: 6pt, radius: 3pt, raw(block: true, lang: {}, {}))",
                            str_arg(&lang.to_lowercase()),
                            str_arg(code)
                        )),
                    }
                }
                BlockKind::Table => Some(self.table(text, b.range.clone())),
                BlockKind::Quote => Some(self.quote(&text[b.range.clone()], line)),
            };
            if let Some(p) = piece {
                // consecutive list items stay one list
                if is_list && prev_list {
                    if let Some(last) = out.last_mut() {
                        last.push('\n');
                        last.push_str(&p);
                    }
                } else {
                    out.push(p);
                }
            }
            prev_list = is_list;
        }
        out.join("\n\n")
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
        for (ri, row) in t.rows.iter().enumerate() {
            for c in 0..cols {
                let content = row.get(c).map(|cell| self.inline(text, cell.clone())).unwrap_or_default();
                cells.push(if ri == 0 { format!("[#strong[{content}]]") } else { format!("[{content}]") });
            }
        }
        let stroke = if self.ctx.no_border { "none" } else { "0.5pt + luma(180)" };
        format!("#table(columns: {cols}, align: ({},), stroke: {stroke}, inset: 5pt, {})", aligns.join(", "), cells.join(", "))
    }

    /// A blockquote or callout (`src` = its lines, `line` = its first line in the note).
    fn quote(&self, src: &str, line: usize) -> String {
        let inner = dequote(src);
        let first = inner.lines().next().unwrap_or("");
        let header = regex::Regex::new(r"^\[!([^\]|]+)(?:\|([^\]]*))?\]([+-]?)\s*(.*)$").expect("regex");
        let Some(c) = header.captures(first.trim_start()) else {
            return format!("#quote(block: true)[\n{}\n]", self.blocks(&inner, line));
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
        let body_t = match (self.ctx.callout_body)(&kind, &meta, &body_lines, &nested) {
            Some(t) => t,
            None => self.blocks(&body, line + 1),
        };
        let end = (self.ctx.body_end)(&kind).map(|m| format!(" #h(1fr) {}", lit(&m))).unwrap_or_default();
        let col = rgb(style.color);
        let label = number.or_else(|| style.label.clone()).unwrap_or_default();
        match style.look {
            CalloutLook::Obsidian => {
                let head = if title_src.trim().is_empty() { lit(&capitalize(&kind)) } else { title };
                format!("#block(width: 100%, inset: 8pt, radius: 4pt, fill: {col}.lighten(90%), stroke: (left: 2pt + {col}))[#text(fill: {col}, weight: \"bold\")[{head}]\n\n{body_t}{end}]")
            }
            CalloutLook::Badge => format!(
                "#block(width: 100%, inset: 8pt, radius: 4pt, fill: {col}.lighten(94%), stroke: 1pt + {col})[#box(fill: {col}, inset: (x: 4pt, y: 2pt), radius: 2pt, text(fill: white, weight: \"bold\")[{}]) {title}\n\n{body_t}{end}]",
                lit(&label)
            ),
            CalloutLook::Soft => format!(
                "#block(width: 100%, inset: (left: 8pt, y: 2pt), stroke: (left: 1.5pt + {col}.lighten(30%)))[#emph[#text(fill: {col}, weight: \"bold\")[{}]] #emph[{title}]\n\n{body_t}{end}]",
                lit(&if label.is_empty() { capitalize(&kind) } else { label.clone() })
            ),
            CalloutLook::Rules => format!(
                "#block(width: 100%, inset: (x: 4pt, y: 6pt), stroke: (top: 1.5pt + {col}, bottom: 1.5pt + {col}))[#text(fill: {col}, weight: \"bold\")[{}] {title}\n\n{body_t}{end}]",
                lit(&label)
            ),
            CalloutLook::Caption { below } => {
                let cap = format!("#strong[{}] {title}", lit(&label));
                if below {
                    format!("#block(width: 100%)[#align(center)[{body_t}]\n\n#align(center)[{cap}]]")
                } else {
                    format!("#block(width: 100%)[#align(center)[{cap}]\n\n#align(center)[{body_t}]]")
                }
            }
        }
    }
}

/// Typst for a MathJax SVG (as the M0 spike did): inline on the baseline, display centred.
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
    let img = format!("image({}, height: {:.3}em)", str_arg(name), h * EX_EM);
    if display {
        format!("#align(center, {img});")
    } else {
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
            style: &|k: &str, _: &str| Style { look: if k == "thm" { CalloutLook::Badge } else { CalloutLook::Obsidian }, color: (40, 100, 110), label: (k == "thm").then(|| "Theorem".into()) },
            code: &|lang: &str, _: &str, _: &dyn Fn(&str) -> String| (lang == "chart").then(|| "#box[CHART]".to_string()),
            callout_body: &|_, _, _, _| None,
            body_end: &|k: &str| (k == "pf").then(|| "□".to_string()),
            embed: &|_| None,
            no_border: false,
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
            let t = "# Title\n## Sec\n> [!thm] Big\n> body $x^2$\n\nsee [[#^e1]] and **bold** ==hi==\n$$ a=b $$\n^e1\n- one\n- [x] two\n\n```chart\ntype: bar\n```\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n> [!pf]\n> done\n";
            let out = e.blocks(t, 0);
            assert!(out.contains("= #\"Title\";"), "{out}");
            assert!(out.contains("== #\"Sec\";"));
            assert!(out.contains("text(fill: white, weight: \"bold\")[#\"Theorem 1.1\";]") && out.contains("#\"Big\";"), "numbered badge: {out}");
            assert!(out.contains("@@M0@@"), "inline math placeholder");
            assert!(out.contains("#\"eq:1.1\";"), "reference label");
            assert!(out.contains("#strong[#\"bold\";];") && out.contains("#highlight"));
            assert!(out.contains("#grid(columns: (1fr, auto)") && out.contains("#\"(1.1)\";"), "numbered equation");
            assert!(out.contains("- #\"one\";\n- ☑ #\"two\";"), "one list: {out}");
            assert!(out.contains("#box[CHART]"));
            assert!(out.contains("#table(columns: 2, align: (left, right,)"));
            assert!(out.contains("#h(1fr) #\"□\";"), "proof end mark");
            assert_eq!(*e.math.borrow(), vec![("x^2".to_string(), false), ("a=b".to_string(), true)]);
        });
    }

    #[test]
    fn math_snippets_follow_mathjax_metrics() {
        let svg = r#"<svg style="vertical-align: -0.566ex;" width="5.2ex" height="2.262ex"></svg>"#;
        assert_eq!(math_snippet("m0.svg", svg, false), "#box(baseline: 0.250em, image(\"m0.svg\", height: 1.000em));");
        assert!(math_snippet("m0.svg", svg, true).starts_with("#align(center, image("));
    }
}
