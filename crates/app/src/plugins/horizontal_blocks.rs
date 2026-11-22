//! Horizontal blocks (compatible with the community plugin "Horizontal Blocks"): a fenced
//! `horizontal` / `hblock` block whose content, split at `---` lines, is shown as side-by-side
//! columns of rendered Markdown. Off by default; it exists to exercise layout plugins against
//! the plugin API (nested Markdown rendering) and, in M5, PDF export.
//!
//! Column widths saved by the original plugin (its `data.json`, read only) are honoured.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::api::*;

/// Splits a block into columns at lines that are exactly `---` (ignoring fenced code inside).
pub fn columns(src: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut fence: Option<String> = None;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            }
        } else if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(t[..3].to_string());
        } else if t == "---" {
            out.push(String::new());
            continue;
        }
        out.last_mut().expect("non-empty").push_str(line);
    }
    out.into_iter().map(|c| c.trim_matches('\n').to_string()).collect()
}

/// Widths (px) saved by the original plugin for this block, by column.
fn saved_widths(data: &Map<String, Value>, src: &str) -> Vec<Option<f32>> {
    let hash: String = Sha256::digest(src.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    let layout = data.get(&format!("horizontal-block-layout-{hash}")).and_then(Value::as_object);
    (0..16).map(|i| layout.and_then(|l| l.get(&format!("width-{i}"))).and_then(Value::as_f64).map(|w| w as f32)).collect()
}

struct Columns {
    borders: bool,
    data: Map<String, Value>,
}

impl CodeBlockRenderer for Columns {
    fn render(&self, code: &str, md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> AnyElement {
        let cols = columns(code);
        let widths = saved_widths(&self.data, code);
        let border = cx.theme().border;
        let n = cols.len();
        let children: Vec<AnyElement> = cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let body = md.render(c, window, cx);
                let col = div().min_w_0().px_2().child(body);
                let col = match widths.get(i).copied().flatten() {
                    Some(w) if i + 1 < n => col.flex_none().w(px(w.max(80.))),
                    _ => col.flex_1(),
                };
                col.when(self.borders && i + 1 < n, |d| d.border_r_1().border_color(border)).into_any_element()
            })
            .collect();
        div().flex().flex_row().w_full().gap_2().py_1().children(children).into_any_element()
    }
}

impl Columns {
    /// `#grid` with one cell per column; saved pixel widths become points, the rest `1fr`.
    fn typst(&self, code: &str, md: &dyn Fn(&str) -> String) -> String {
        let cols = columns(code);
        let widths = saved_widths(&self.data, code);
        let n = cols.len();
        let spec: Vec<String> = (0..n).map(|i| match widths.get(i).copied().flatten() {
            Some(w) if i + 1 < n => format!("{:.0}pt", w.max(80.) * 0.75),
            _ => "1fr".to_string(),
        }).collect();
        let stroke = if self.borders { format!(",\n  stroke: (x, y) => if x < {} {{ (right: 0.5pt + luma(180)) }}", n - 1) } else { String::new() };
        let cells: Vec<String> = cols.iter().map(|c| format!("[\n{}\n]", md(c))).collect();
        format!("#grid(\n  columns: ({}),\n  column-gutter: 10pt,\n  inset: (x: 4pt){stroke},\n  {}\n)", spec.join(", "), cells.join(",\n  "))
    }
}

pub struct HorizontalBlocks {
    manifest: PluginManifest,
    vault: PathBuf,
}

impl HorizontalBlocks {
    pub fn new(vault: &Path) -> Self {
        Self {
            manifest: PluginManifest {
                id: "horizontal-blocks",
                name: "Horizontal blocks",
                version: "0.1.0",
                description: "Side-by-side columns from ```horizontal / ```hblock blocks split at --- lines (compatible with the Horizontal Blocks community plugin).",
                enabled_by_default: false,
                settings: vec![SettingSpec { key: "borders", label: "Column dividers", description: "Draw a line between columns.", kind: SettingKind::Toggle, default: Value::Bool(true) }],
                code_languages: &["horizontal", "hblock"],
            },
            vault: vault.to_path_buf(),
        }
    }
}

impl Plugin for HorizontalBlocks {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let data: Map<String, Value> = std::fs::read_to_string(self.vault.join(".obsidian/plugins/horizontal-blocks/data.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let borders = settings.get("borders").and_then(Value::as_bool).unwrap_or(true);
        let r = Rc::new(Columns { borders, data });
        for lang in ["horizontal", "hblock"] {
            reg.code_block_renderer(lang, SharedColumns(r.clone()));
        }
    }
}

struct SharedColumns(Rc<Columns>);

impl CodeBlockRenderer for SharedColumns {
    fn render(&self, code: &str, md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> AnyElement {
        self.0.render(code, md, window, cx)
    }
    fn export_typst(&self, code: &str, md: &dyn Fn(&str) -> String) -> Option<String> {
        Some(self.0.typst(code, md))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_at_rules_outside_code() {
        let c = super::columns("### A\nleft\n---\n```\n---\n```\n---\nright\n");
        assert_eq!(c, vec!["### A\nleft".to_string(), "```\n---\n```".to_string(), "right".to_string()]);
    }

    #[test]
    fn exports_a_typst_grid() {
        let c = super::Columns { borders: true, data: serde_json::Map::new() };
        let t = c.typst("a\n---\nb\n---\nc", &|m: &str| format!("<{m}>"));
        assert!(t.starts_with("#grid(") && t.contains("columns: (1fr, 1fr, 1fr)"), "{t}");
        assert!(t.contains("[\n<a>\n]") && t.contains("[\n<c>\n]"));
        assert!(t.contains("if x < 2"));
        // the spike's Typst compiler checks this output (see docs/milestones/M2-feedback.md)
        if let Ok(dir) = std::env::var("OBSIDILIGHT_TYPST_OUT") {
            let body = c.typst("=== Left\nSome *bold* text and $e^(i pi) + 1 = 0$.\n\n$ integral_0^1 x dif x $\n---\n=== Right\n- a list\n- another item", &|m: &str| m.to_string());
            std::fs::write(std::path::Path::new(&dir).join("main.typ"), format!("#set page(paper: \"a4\", margin: 10mm)\n= Layout\n\n{body}\n\nAfter the block.\n")).unwrap();
        }
    }

    #[test]
    fn reads_the_original_plugins_widths() {
        let mut layout = serde_json::Map::new();
        let src = "a\n---\nb";
        let hash: String = <sha2::Sha256 as sha2::Digest>::digest(src.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
        layout.insert(format!("horizontal-block-layout-{hash}"), serde_json::json!({"width-0": 240.0}));
        assert_eq!(super::saved_widths(&layout, src)[..2], [Some(240.0), None]);
    }
}
