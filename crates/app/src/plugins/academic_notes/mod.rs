//! Academic Notes (replaces the community plugin of the same name; reimplemented from its
//! observed behaviour, none of its code): callout styling for its environments (D7) with the
//! vault's palettes, numbering per section, references (`[[#^lemma1]]` shows "lem 1.1"),
//! algorithm pseudocode with line numbers, and the end-of-proof mark.

pub mod algorithm;
pub mod numbering;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

/// Environment groups and their aliases, as in the plugin's stylesheet.
const ENVS: &[(&str, &str, &[&str])] = &[
    ("def", "Definition", &["def", "definition", "asm", "assumption"]),
    ("thm", "Theorem", &["thm", "theorem", "axiom", "axm", "hyp", "hypothesis"]),
    ("lem", "Lemma", &["lem", "lemma"]),
    ("prop", "Proposition", &["prop", "prp", "proposition"]),
    ("cor", "Corollary", &["cor", "corollary"]),
    ("claim", "Claim", &["claim", "clm", "conjecture", "cnj"]),
    ("example", "Example", &["example", "exm", "exercise", "exr", "ex", "exa"]),
    ("solution", "Solution", &["solution", "sol"]),
    ("proof", "Proof", &["proof", "pf"]),
    ("remark", "Remark", &["remark", "rem", "rmk"]),
    ("algorithm", "Algorithm", &["algorithm", "alg"]),
];

/// Display names of aliases that differ from their group's (e.g. `axiom` -> Axiom).
fn alias_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "asm" | "assumption" => "Assumption",
        "axiom" | "axm" => "Axiom",
        "hyp" | "hypothesis" => "Hypothesis",
        "conjecture" | "cnj" => "Conjecture",
        "exercise" | "exr" => "Exercise",
        _ => return None,
    })
}

/// (def, thm, lem, prop, cor, claim, example, proof) accents per palette.
fn palette(name: &str) -> [u32; 8] {
    match name {
        "colorful-dark" => [0x86ca98, 0x83bbce, 0xb7a7ca, 0xb8c984, 0x8dcbb4, 0x9baed0, 0xdec178, 0xa8b898],
        "colorful" => [0x247651, 0x286b76, 0x71628c, 0x62752e, 0x42786b, 0x44698c, 0x946b2f, 0x687252],
        _ /* forest */ => [0x2e7055, 0x296a70, 0x537354, 0x73723d, 0x367d69, 0x477486, 0x87653e, 0x5c6d61],
    }
}

struct Styles {
    light: [u32; 8],
    dark: [u32; 8],
    numbers: Rc<NumberCache>,
    /// Number pseudocode lines unless a block says `lines=false` (Academic Notes' setting).
    line_numbers: bool,
}

/// Numbers of the last text asked about (the editor asks again on every change).
#[derive(Default)]
struct NumberCache(RefCell<(u64, Rc<Vec<numbering::Numbered>>)>);

impl NumberCache {
    fn of_text(&self, text: &str) -> Rc<Vec<numbering::Numbered>> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        let key = h.finish();
        let mut c = self.0.borrow_mut();
        if c.0 != key || c.1.is_empty() && !text.is_empty() {
            *c = (key, Rc::new(numbering::number(&numbering::items_of_text(text))));
        }
        c.1.clone()
    }
}

/// The pseudocode of an algorithm callout (lines numbered with `|lines=true`).
pub fn algorithm_numbered(meta: &str, default: bool) -> bool {
    let has = |v: &str| meta.split(['|', ',', ' ']).any(|m| m.trim() == v);
    if has("lines=false") { false } else { default || has("lines=true") }
}

fn render_algorithm(meta: &str, body: &[String], line_numbers: bool, md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> AnyElement {
    let numbered = algorithm_numbered(meta, line_numbers);
    // block ids at the end (`^alg1`) are not pseudocode
    let src: Vec<&str> = body.iter().map(String::as_str).filter(|l| !(l.trim().starts_with('^') && !l.trim().contains(' '))).collect();
    let muted = cx.theme().muted_foreground;
    let rows: Vec<AnyElement> = algorithm::parse(&src)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let line_md: Vec<String> = l.parts.iter().map(|p| match p {
                algorithm::Part::Keyword(k) => format!("**{k}**"),
                algorithm::Part::Text(t) => t.clone(),
            }).collect();
            let content = md.render(&line_md.join(" "), window, cx);
            div()
                .flex()
                .items_start()
                .gap_2()
                .when(numbered, |d| d.child(div().w(px(22.)).flex_none().text_right().text_sm().text_color(muted).child((i + 1).to_string())))
                .child(div().pl(px(18. * l.depth as f32)).flex_1().min_w_0().child(content))
                .into_any_element()
        })
        .collect();
    div().flex().flex_col().children(rows).into_any_element()
}

impl CalloutRenderer for Styles {
    fn numbers(&self, text: &str) -> HashMap<usize, String> {
        self.numbers.of_text(text).iter().map(|n| (n.line, n.label.clone())).collect()
    }

    fn render_body(&self, kind: &str, meta: &str, body: &[String], md: &MarkdownRenderer, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        matches!(kind, "algorithm" | "alg").then(|| render_algorithm(meta, body, self.line_numbers, md, window, cx))
    }

    fn body_end(&self, kind: &str) -> Option<String> {
        matches!(kind, "proof" | "pf").then(|| "\u{25A1}".to_string())
    }

    fn style(&self, kind: &str, _meta: &str, dark: bool) -> Option<CalloutStyle> {
        let (group, label, _) = ENVS.iter().find(|(_, _, aliases)| aliases.contains(&kind))?;
        let p = if dark { &self.dark } else { &self.light };
        let ix = match *group {
            "def" | "algorithm" => 0,
            "thm" => 1,
            "lem" => 2,
            "prop" => 3,
            "cor" => 4,
            "claim" => 5,
            "example" => 6,
            _ => 7, // solution, proof, remark
        };
        let look = match *group {
            "proof" | "remark" => CalloutLook::Soft,
            "algorithm" => CalloutLook::Rules,
            _ => CalloutLook::Badge,
        };
        let label = alias_label(kind).unwrap_or(label);
        Some(CalloutStyle { color: rgb(p[ix]).into(), look, icon: None, label: Some(label.to_string()) })
    }
}

/// Entries of the Contents panel: (depth, text, offset, is a numbered environment).
pub fn contents(text: &str, depth: u8) -> Vec<(u8, String, usize, bool)> {
    use obsidilight_core::markdown::{parse_blocks, BlockKind};
    let numbers = numbering::number(&numbering::items_of_text(text));
    let mut out = Vec::new();
    let mut level = 0u8;
    let mut line = 0usize;
    let mut last = 0usize;
    for b in parse_blocks(text) {
        line += text[last..b.range.start].matches('\n').count();
        last = b.range.start;
        match b.kind {
            BlockKind::Heading { level: l } if l <= depth => {
                level = l;
                out.push((l, text[b.content.clone()].trim().to_string(), b.range.start, false));
            }
            BlockKind::Heading { level: l } => level = l,
            BlockKind::Quote => {
                let end_line = line + text[b.range.clone()].matches('\n').count();
                for n in numbers.iter().filter(|n| n.line >= line && n.line <= end_line) {
                    let off = text.split_inclusive('\n').take(n.line).map(str::len).sum();
                    let title = text.lines().nth(n.line).and_then(|h| h.split_once(']')).map(|(_, t)| t.trim_start_matches(['-', '+']).trim().to_string()).unwrap_or_default();
                    let shown = if title.is_empty() { n.label.clone() } else { format!("{} · {title}", n.label) };
                    out.push((level.saturating_add(1).min(7), shown, off, true));
                }
            }
            _ => {}
        }
    }
    out
}

/// `[[#^id]]` / `[[Note#^id]]` pointing into a numbered callout shows its short label.
struct References {
    numbers: Rc<NumberCache>,
}

impl LinkLabeler for References {
    fn label(&self, target: &LinkTarget, sub: &str) -> Option<String> {
        let id = sub.strip_prefix('^')?;
        let (numbers, line) = match target {
            LinkTarget::Text(t) => {
                let line = t.lines().position(|l| {
                    let l = l.trim_end();
                    l.ends_with(&format!("^{id}")) && l[..l.len() - id.len() - 1].chars().next_back().is_none_or(|c| c.is_whitespace() || c == '>')
                })?;
                (self.numbers.of_text(t), line)
            }
            LinkTarget::Meta(m) => {
                let line = m.block_ids.iter().find(|(b, _)| b == id)?.1;
                (Rc::new(numbering::number(&numbering::items_of_meta(m))), line)
            }
        };
        numbering::at_line(&numbers, line).map(|n| n.short.clone())
    }
}

pub struct AcademicNotes {
    manifest: PluginManifest,
}

impl AcademicNotes {
    /// Defaults come from the vault's Academic Notes settings (`data.json`, read only).
    pub fn new(vault: &std::path::Path) -> Self {
        let an: Map<String, Value> = std::fs::read_to_string(vault.join(".obsidian/plugins/academic-notes/data.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let s = |k: &str, d: &str| Value::from(an.get(k).and_then(Value::as_str).unwrap_or(d));
        let b = |k: &str, d: bool| Value::Bool(an.get(k).and_then(Value::as_bool).unwrap_or(d));
        let palettes = vec![("forest", "Forest"), ("colorful", "Colorful"), ("colorful-dark", "Colorful (dark)")];
        Self {
            manifest: PluginManifest {
                id: "academic-notes",
                name: "Academic Notes",
                version: "0.1.0",
                description: "Theorem-like environments (definition, theorem, proof, …) as styled callouts. Numbering and references arrive in M3.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "lightPalette", label: "Light palette", description: "Accent colours in the light theme.", kind: SettingKind::Choice(palettes.clone()), default: s("lightPalette", "forest") },
                    SettingSpec { key: "darkPalette", label: "Dark palette", description: "Accent colours in the dark theme.", kind: SettingKind::Choice(palettes), default: s("darkPalette", "colorful-dark") },
                    SettingSpec { key: "tocDepth", label: "Contents depth", description: "Deepest heading level shown in the Contents panel.", kind: SettingKind::Number, default: Value::from(an.get("tocDepth").and_then(Value::as_u64).unwrap_or(3)) },
                    SettingSpec { key: "algorithmLineNumbers", label: "Number algorithm lines", description: "Line numbers in pseudocode (a block can say lines=false).", kind: SettingKind::Toggle, default: b("algorithmLineNumbers", true) },
                ],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for AcademicNotes {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str, d: &str| settings.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let numbers = Rc::new(NumberCache::default());
        reg.callout_renderer(Styles { light: palette(&get("lightPalette", "forest")), dark: palette(&get("darkPalette", "colorful-dark")), numbers: numbers.clone(), line_numbers: settings.get("algorithmLineNumbers").and_then(Value::as_bool).unwrap_or(true) });
        reg.link_labeler(Rc::new(References { numbers }));
        let depth = settings.get("tocDepth").and_then(Value::as_f64).unwrap_or(3.0).clamp(1.0, 6.0) as u8;
        reg.sidebar_view(SidebarView {
            id: "contents".into(),
            title: "Contents".into(),
            icon: gpui_kit::assets::IconName::ListOrdered,
            render: Rc::new(move |editor, _window, cx| {
                let Some(editor) = editor.cloned() else { return div().text_sm().child("No note open").into_any_element() };
                let text = editor.read(cx).doc.text();
                let muted = cx.theme().muted_foreground;
                let rows: Vec<AnyElement> = contents(&text, depth)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (d, label, off, env))| {
                        let editor = editor.clone();
                        div()
                            .id(("toc", i))
                            .pl(px(12. * d.saturating_sub(1) as f32))
                            .py_0p5()
                            .text_sm()
                            .cursor_pointer()
                            .when(env, |e| e.text_color(muted).text_xs())
                            .when(!env && d <= 2, |e| e.font_weight(FontWeight::SEMIBOLD))
                            .child(label)
                            .on_click(move |_, window, cx| {
                                editor.update(cx, |e, cx| {
                                    window.focus(&e.focus, cx);
                                    e.set_selection(obsidilight_core::Selection::caret(off), false, cx);
                                });
                            })
                            .into_any_element()
                    })
                    .collect();
                div().id("contents").flex().flex_col().size_full().overflow_y_scroll().children(rows).into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{palette, CalloutLook, CalloutRenderer, LinkLabeler, LinkTarget, References, Styles};
    use gpui_kit::rgb;

    #[test]
    fn claims_its_environments_only() {
        let s = Styles { light: palette("forest"), dark: palette("colorful-dark"), numbers: Default::default(), line_numbers: true };
        let thm = s.style("thm", "", false).unwrap();
        assert_eq!((thm.look, thm.label.as_deref()), (CalloutLook::Badge, Some("Theorem")));
        assert_eq!(thm.color, rgb(0x296a70).into());
        assert_eq!(s.style("thm", "", true).unwrap().color, rgb(0x83bbce).into());
        assert_eq!(s.style("pf", "", false).unwrap().look, CalloutLook::Soft);
        assert_eq!(s.style("axiom", "", false).unwrap().label.as_deref(), Some("Axiom"));
        assert_eq!(s.style("algorithm", "lines=true", false).unwrap().look, CalloutLook::Rules);
        assert!(s.style("note", "", false).is_none(), "Obsidian's own types stay with the core");
        assert!(s.style("warning", "", false).is_none());
        let t = "## A\n> [!thm] x\n\n> [!lemma]\n> y\n> ^lemma1\n\nsee [[#^lemma1]]\n";
        let n = s.numbers(t);
        assert_eq!((n.get(&1).map(String::as_str), n.get(&3).map(String::as_str)), (Some("Theorem 1.1"), Some("Lemma 1.1")));
        let r = References { numbers: Default::default() };
        assert_eq!(r.label(&LinkTarget::Text(t), "^lemma1").as_deref(), Some("lem 1.1"));
        assert_eq!(r.label(&LinkTarget::Text(t), "^nope"), None);
        assert_eq!(r.label(&LinkTarget::Text(t), "A"), None, "heading links keep their text");
        let c: Vec<(u8, String)> = super::contents("# T\n## A\n> [!thm] Big\n> x\n#### deep\n> [!lem]\n> y\n", 3).into_iter().map(|(d, s, _, _)| (d, s)).collect();
        assert_eq!(c, vec![(1, "T".into()), (2, "A".into()), (3, "Theorem 1.1 · Big".into()), (5, "Lemma 1.1".into())]);
    }
}

/// Pseudocode keywords and line counts against Academic Notes' rendering of the fixture
/// (`reports/m3-reference.jsonl`; skipped when absent).
#[cfg(test)]
mod algorithm_reference {
    use super::algorithm::{parse, Part};
    use obsidilight_core::markdown::{callouts::flatten, parse_blocks, parse_quote, BlockKind};

    #[test]
    fn pseudocode_matches_academic_notes() {
        let report = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/m3-reference.jsonl");
        let Ok(lines) = std::fs::read_to_string(&report) else { return eprintln!("no reference report: skipped") };
        let fixture = std::path::Path::new(&std::env::var("HOME").unwrap()).join("dev/vault-fixture");
        let (mut algs, mut wrong) = (0, Vec::new());
        for l in lines.lines() {
            let Ok(d) = serde_json::from_str::<serde_json::Value>(l) else { continue };
            let note = d["note"].as_str().unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(fixture.join(note)) else { continue };
            let strs = |k: &str| -> Vec<String> {
                d["sections"].as_array().into_iter().flatten().flat_map(|s| s["an"][k].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_str().map(|s| s.trim().to_string())).collect()
            };
            let (their_kw, their_nums) = (strs("an-algorithm-keyword"), strs("an-algorithm-line-number").len());
            let (mut our_kw, mut our_nums) = (Vec::new(), 0);
            for b in parse_blocks(&text).into_iter().filter(|b| b.kind == BlockKind::Quote) {
                for c in flatten(&parse_quote(&text, b.range.clone())) {
                    if c.kind != "algorithm" {
                        continue;
                    }
                    algs += 1;
                    let body: Vec<String> = text[c.header.end..c.range.end].lines().skip(1).map(|l| l.trim_start_matches(|ch: char| ch == '>' || ch == ' ').to_string()).collect();
                    let src: Vec<&str> = body.iter().map(String::as_str).filter(|l| !(l.trim().starts_with('^') && !l.trim().contains(' '))).collect();
                    let parsed = parse(&src);
                    if super::algorithm_numbered(&c.meta, true) {
                        our_nums += parsed.len();
                    }
                    our_kw.extend(parsed.iter().flat_map(|l| l.parts.iter().filter_map(|p| if let Part::Keyword(k) = p { Some(k.clone()) } else { None })));
                }
            }
            if our_kw != their_kw {
                wrong.push(format!("{note}: keywords {our_kw:?} vs {their_kw:?}"));
            }
            if our_nums != their_nums {
                wrong.push(format!("{note}: {our_nums} numbered lines vs {their_nums}"));
            }
        }
        eprintln!("{algs} algorithms compared");
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
