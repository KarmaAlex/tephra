//! Snippet engine tests in a headless editor (HANDOFF §7 "snippets test"). `typed()` replays
//! keys the way the editor does: hooks first, the default action otherwise.

use super::*;
use crate::editor::api::{Backend as _, Headless};
use obsidilight_core::Selection;

/// The vault's own snippets file (read only; the test is skipped when the fixture is absent).
fn fixture_snippets() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    std::fs::read_to_string(Path::new(&home).join("dev/vault-fixture/math-jax-snippets/snippets.js")).ok()
}

fn fixture_settings() -> Map<String, Value> {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let ls = LatexSuite::new(&Path::new(&home).join("dev/vault-fixture"));
    ls.manifest.settings.iter().map(|s| (s.key.to_string(), s.default.clone())).collect()
}

fn engine_with(src: &str) -> Engine {
    let mut m = fixture_settings();
    m.insert("snippetsFileLocation".into(), Value::from("snippets.js"));
    let src = src.to_string();
    Engine::load(&m, &move |rel| (rel == "snippets.js").then(|| src.clone()))
}

/// `|` = caret, `[`…`]` = selection.
fn doc(marked: &str) -> Headless {
    let (text, sel) = if let Some(a) = marked.find('[') {
        let b = marked.find(']').unwrap();
        (format!("{}{}{}", &marked[..a], &marked[a + 1..b], &marked[b + 1..]), Selection::new(a, b - 1))
    } else {
        let a = marked.find('|').unwrap();
        (marked.replacen('|', "", 1), Selection::caret(a))
    };
    let mut h = Headless::new(&text);
    h.doc.set_selection(sel);
    h
}

/// Replays keys: plain characters, or `<Tab>`, `<S-Tab>`, `<CR>`, `<S-CR>`, `<BS>`.
fn typed(e: &Engine, h: &mut Headless, keys: &str) {
    let mut rest = keys;
    while !rest.is_empty() {
        let special = ["<Tab>", "<S-Tab>", "<CR>", "<S-CR>", "<BS>"].iter().find(|k| rest.starts_with(**k)).copied();
        if let Some(k) = special {
            rest = &rest[k.len()..];
            let key = match k {
                "<Tab>" => EditorKey::Tab,
                "<S-Tab>" => EditorKey::ShiftTab,
                "<CR>" => EditorKey::Enter,
                "<S-CR>" => EditorKey::ShiftEnter,
                _ => EditorKey::Backspace,
            };
            if !e.on_key(&mut EditorApi::new(h), key) {
                let r = h.doc.selection().range();
                match key {
                    EditorKey::Tab => h.edit(r, "\t", EditKind::Insert),
                    EditorKey::Enter | EditorKey::ShiftEnter => h.edit(r, "\n", EditKind::Insert),
                    EditorKey::Backspace => {
                        let r = if r.is_empty() { h.doc.prev_grapheme(r.start)..r.start } else { r };
                        h.edit(r, "", EditKind::Delete)
                    }
                    EditorKey::ShiftTab => {}
                }
            }
            continue;
        }
        let c = rest.chars().next().unwrap();
        rest = &rest[c.len_utf8()..];
        let s = c.to_string();
        if !e.on_char(&mut EditorApi::new(h), &s) {
            let r = h.doc.selection().range();
            h.edit(r, &s, EditKind::Insert);
        }
    }
}

fn run(e: &Engine, start: &str, keys: &str) -> String {
    let mut h = doc(start);
    typed(e, &mut h, keys);
    h.marked()
}

#[test]
fn the_vaults_snippets_file_loads_completely() {
    let Some(src) = fixture_snippets() else { return eprintln!("fixture absent: skipped") };
    let e = engine_with(&src);
    assert!(e.errors.is_empty(), "load problems: {:?}", e.errors);
    let js = js::Js::new().unwrap();
    let descs = js.load_snippets(&src).unwrap();
    assert!(descs.len() > 200, "{} snippets", descs.len());
    assert_eq!(e.snippets.len(), descs.len(), "every snippet registers");
    let fallback = e.snippets.iter().filter(|s| matches!(&s.trigger, Trigger::Regex { re: None, .. })).count();
    assert_eq!(fallback, 0, "all regex triggers compile natively");
}

#[test]
fn the_vaults_snippets_expand() {
    let Some(src) = fixture_snippets() else { return eprintln!("fixture absent: skipped") };
    let e = engine_with(&src);
    let m = |s: &str| format!("$$\n{s}\n$$");
    // text mode: inline math (the "${}$0{}$" trick) and display math via the dm function snippet
    assert_eq!(run(&e, "a |", "mk"), "a ${}|{}$");
    assert_eq!(run(&e, "|", "dm"), "$$\n|\n$$");
    assert_eq!(run(&e, "> |", "dm"), "> \n> $$\n> |\n> $$", "callout prefix kept on new lines");
    // plain, regex and function snippets
    assert_eq!(run(&e, &m("|"), "@a"), m("\\alpha|"));
    assert_eq!(run(&e, &m("|"), "xsr"), m("x^{2}|"));
    assert_eq!(run(&e, &m("|"), "x3"), m("x_{3}|"));
    assert_eq!(run(&e, &m("|"), "@a3"), m("\\alpha_{3}|"));
    assert_eq!(run(&e, &m("|"), "iden2"), m("\\begin{pmatrix}\n1 & 0 \\\\\n0 & 1\n\\end{pmatrix}|"));
    // typing a macro is left alone; an unknown continuation gets a space
    assert_eq!(run(&e, &m("|"), "\\alp"), m("\\alp|"));
    assert_eq!(run(&e, &m("|"), "\\tox"), m("\\to x|"));
    // tabstops
    assert_eq!(run(&e, &m("|"), "//1<Tab>2<Tab>"), m("\\frac{1}{2}|"));
    assert_eq!(run(&e, &m("|"), "sq"), m("\\sqrt{ | }"));
    assert_eq!(run(&e, &m("|"), "dint"), m("\\int_{[0]}^{1}  \\, dx "));
    assert_eq!(run(&e, &m("|"), "dint<Tab>"), m("\\int_{0}^{[1]}  \\, dx "));
    assert_eq!(run(&e, &m("|"), "taylg"), m("g|(x + h) = g(x) + g'(x)h + g''(x) \\frac{h^{2}}{2!} + \\dots"), "mirrored placeholder");
    // non-automatic snippet on Tab
    assert_eq!(run(&e, &m("|"), "par<Tab>"), m("\\frac{ \\partial [y] }{ \\partial x } "));
    // environments: regex with captures, mirrored names, matrix shortcuts
    assert_eq!(run(&e, &m("|"), "pmat"), m("\\begin{pmatrix}\n|\n\\end{pmatrix}"));
    assert_eq!(run(&e, &m("|"), "pmata<Tab>b<CR>c"), m("\\begin{pmatrix}\na & b \\\\\nc|\n\\end{pmatrix}"));
    assert_eq!(run(&e, &m("|"), "pmata<S-CR>"), m("\\begin{pmatrix}\na\n\\end{pmatrix}|"));
    // visual snippet on a selection
    assert_eq!(run(&e, &m("[x+y]"), "U"), m("\\underbrace{ x+y }_{ | }"));
    // inline math trims trailing whitespace of expansions
    assert_eq!(run(&e, "$x|$", "lim"), "$x\\lim_{ [n] \\to \\infty }$");
}

#[test]
fn editor_features() {
    let e = engine_with("export default []");
    let m = |s: &str| format!("$$\n{s}\n$$");
    // auto-fraction
    assert_eq!(run(&e, "$$\nx+ab|\n$$", "/"), m("x+\\frac{ab}{|}"));
    assert_eq!(run(&e, "$$\n(a+b)|\n$$", "/"), m("\\frac{a+b}{|}"));
    assert_eq!(run(&e, "$$\n\\alpha b|\n$$", "/"), m("\\frac{\\alpha b}{|}"), "space after a Greek letter");
    assert_eq!(run(&e, "$$\nx^{a|}\n$$", "/"), m("x^{a/|}"), "excluded inside ^{{}}");
    assert_eq!(run(&e, "a|", "/"), "a/|", "text mode");
    // tabout
    assert_eq!(run(&e, &m("\\sqrt{ x| }"), "<Tab>"), m("\\sqrt{ x }|"));
    assert_eq!(run(&e, "$x|$ y", "<Tab>"), "$x$| y");
    assert_eq!(run(&e, &m("x|"), "<Tab>"), "$$\nx\n$$\n|", "leaves multi-line math onto a new line");
    assert_eq!(run(&e, &m("\\frac{a}{b|}"), "<S-Tab>"), m("\\frac{a}|{b}"));
    assert_eq!(run(&e, &m("(a|)"), ")"), m("(a)|"), "typing a closing bracket steps over it");
    assert_eq!(run(&e, "a|", "<Tab>"), "a\t|", "outside math Tab is a tab");
    // auto-delete $
    assert_eq!(run(&e, "a $|$ b", "<BS>"), "a | b");
    assert_eq!(run(&e, "a $x|$ b", "<BS>"), "a $|$ b");
}

#[test]
fn auto_enlarge_and_undo() {
    let e = engine_with(r#"export default [ {trigger: "sum", replacement: "\\sum", options: "mA"}, {trigger: "sr", replacement: "^{2}", options: "mA"} ]"#);
    assert_eq!(run(&e, "$$\n(|)\n$$", "sum"), "$$\n\\left( \\sum| \\right)\n$$");
    let mut h = doc("$$\nx|\n$$");
    typed(&e, &mut h, "sr");
    assert_eq!(h.marked(), "$$\nx^{2}|\n$$");
    assert!(h.doc.undo());
    assert_eq!(h.doc.text(), "$$\nxsr\n$$", "undo restores the typed trigger");
}

#[test]
fn builtin_defaults_load() {
    let mut m = Map::new();
    m.insert("snippetsFileLocation".into(), Value::from(""));
    let e = Engine::load(&m, &|_| None);
    assert!(e.errors.is_empty(), "{:?}", e.errors);
    assert!(e.snippets.len() > 150);
    assert_eq!(run(&e, "$$\n|\n$$", "@b"), "$$\n\\beta|\n$$");
}

#[test]
fn snippet_keys_reach_the_headless_backend() {
    // sanity: the harness edits through the same backend the editor uses
    let mut h = doc("ab|");
    h.edit(2..2, "c", EditKind::Insert);
    assert_eq!(h.marked(), "abc|");
    h.select(Selection::caret(0));
    assert_eq!(h.marked(), "|abc");
}

#[test]
fn inline_math_snippet_brackets_and_fractions() {
    // the vault's own snippets for these keys, so the test runs without the fixture
    let e = engine_with(
        r#"export default [
        {trigger: "mk", replacement: "${}$0{}$", options: "tA"},
        {trigger: "(", replacement: "($0)$1", options: "mA"},
        {trigger: "sum", replacement: "\\sum", options: "mA"},
    ]"#,
    );
    // backspace in an empty inline formula removes all of it
    assert_eq!(run(&e, "a |", "mk<BS>"), "a |");
    assert_eq!(run(&e, "a |", "mkx<BS><BS>"), "a |");
    // Tab at the end leaves the formula, past the trailing {}
    assert_eq!(run(&e, "a |", "mkx<Tab>"), "a ${}x{}$|");
    assert_eq!(run(&e, "$$\nx^{|}\n$$", "<Tab>"), "$$\nx^{}|\n$$", "a {{}} around the caret is still left");
    // auto-fraction leaves the leading {} alone
    assert_eq!(run(&e, "a |", "mkab/"), "a ${}\\frac{ab}{|}{}$");
    // enlarged brackets: Tab leaves the fraction, then the brackets, then the formula
    assert_eq!(run(&e, "a |", "mk(1-1/k<Tab>"), "a ${}\\left( 1-\\frac{1}{k}| \\right){}$");
    assert_eq!(run(&e, "a |", "mk(1-1/k<Tab><Tab>"), "a ${}\\left( 1-\\frac{1}{k} \\right)|{}$");
    assert_eq!(run(&e, "a |", "mk(1-1/k<Tab><Tab><Tab>"), "a ${}\\left( 1-\\frac{1}{k} \\right){}$|");
    // typing ) before an enlarged \right) steps over it
    assert_eq!(run(&e, "a |", "mk(sum 1/k<Tab>)"), "a ${}\\left( \\sum \\frac{1}{k} \\right)|{}$");
    // \left( … \right) around a numerator goes, like ( … )
    assert_eq!(run(&e, "a |", "mk(sum 1/k<Tab><Tab>/"), "a ${}\\frac{\\sum \\frac{1}{k}}{|}{}$");
    assert_eq!(run(&e, "$$\n\\left( a \\right)\\left( b \\right)|\n$$", "/"), "$$\n\\frac{\\left( a \\right)\\left( b \\right)}{|}\n$$", "two groups keep theirs");
    // and Tab keeps going forward, never back into the replaced fraction's tabstops
    assert_eq!(run(&e, "a |", "mk(sum 1/k<Tab>)/k<Tab>"), "a ${}\\frac{\\sum \\frac{1}{k}}{k}|{}$");
    assert_eq!(run(&e, "a |", "mk(sum 1/k<Tab>)/k<Tab><Tab>"), "a ${}\\frac{\\sum \\frac{1}{k}}{k}{}$|");
}

#[test]
fn inline_math_preview_finds_the_formula_under_the_caret() {
    use features::inline_math_preview as prev;
    let t = "text $a+b$ more $$x$$ end\n";
    let at = |s: &str| t.find(s).unwrap();
    // inside, at either edge of the inner text
    assert_eq!(prev(t, at("a+b")), Some(("$a+b$".to_string(), at("$a+b$")..at("$a+b$") + 5)));
    assert_eq!(prev(t, at("+b")).map(|p| p.0), Some("$a+b$".into()));
    assert_eq!(prev(t, at("$ more")).map(|p| p.0), Some("$a+b$".into()));
    // outside math, before the opening `$`, and in inline display math
    assert_eq!(prev(t, at("text")), None);
    assert_eq!(prev(t, at("$a+b$")), None);
    assert_eq!(prev(t, at("x$$")), None);
    // math blocks have their own preview
    let b = "$$\nx^2\n$$\n";
    assert_eq!(prev(b, b.find("x^2").unwrap()), None);
    // empty formulas: nothing to show
    let e = "a $ $ b $$ c";
    assert_eq!(prev(e, 3), None);
}

#[test]
fn inline_math_preview_in_callouts_and_table_cells() {
    use features::inline_math_preview as prev;
    let c = "> [!thm] T\n> > where $a + b$ holds\n";
    assert_eq!(prev(c, c.find("a +").unwrap()).map(|p| p.0), Some("$a + b$".into()));
    let t = "| x | y |\n|---|---|\n| $\\|v\\|$ | 2 |\n";
    assert_eq!(prev(t, t.find("v").unwrap()).map(|p| p.0), Some("$|v|$".into()));
}

#[test]
fn math_preview_setting_registers_and_unregisters_the_popup() {
    use crate::plugins::{Plugin as _, Registrar, Registry};
    let home = std::env::var_os("HOME").unwrap_or_default();
    for (on, want) in [(true, 1), (false, 0)] {
        let mut ls = LatexSuite::new(&Path::new(&home).join("dev/vault-fixture"));
        let mut m = fixture_settings();
        m.insert("mathPreviewEnabled".into(), Value::Bool(on));
        let mut reg = Registry::default();
        ls.on_load(&m, &mut Registrar { plugin: "latex-suite", reg: &mut reg });
        assert_eq!(reg.caret_popups.len(), want, "mathPreviewEnabled = {on}");
        if on {
            let p = reg.caret_popups[0].1.popup("x $y$", 3).expect("popup in inline math");
            assert_eq!((p.markdown.as_str(), p.anchor.clone()), ("$y$", 2..5));
        }
        reg.remove_plugin("latex-suite");
        assert_eq!(reg.count("latex-suite"), 0);
    }
}
