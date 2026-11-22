//! Spell check (Obsidian's core spell checker): Hunspell dictionaries for any number of
//! languages (a word is right if any of them accepts it), misspellings underlined, suggestions
//! and "Add to dictionary" in the editor's right-click menu.
//!
//! Dictionaries are the system's Hunspell ones (`hunspell-en_us`, `hunspell-it`, … packages:
//! `/usr/share/hunspell`), `~/.local/share/hunspell`, `$DICPATH`, or `<vault>/.obsidilight/
//! dictionaries`. Checking uses `spellbook` (pure-Rust Hunspell). Words you add are kept in
//! `<vault>/.obsidilight/dictionary.txt`, one per line.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui_kit::{div, App, Hsla, IntoElement as _, ParentElement as _, Styled as _};
use obsidilight_core::markdown::{parse_blocks, parse_inline, BlockKind, Inline, InlineKind};
use obsidilight_core::{EditKind, Selection};
use serde_json::{Map, Value};
use spellbook::Dictionary;

use super::api::*;
use crate::editor::Editor;

/// Folders searched for `<lang>.aff` + `<lang>.dic`.
pub fn search_dirs(vault: &Path, extra: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = extra.split([',', ':']).map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    v.push(vault.join(".obsidilight/dictionaries"));
    if let Some(p) = std::env::var_os("DICPATH") {
        v.extend(std::env::split_paths(&p));
    }
    if let Some(h) = std::env::var_os("HOME") {
        v.push(Path::new(&h).join(".local/share/hunspell"));
    }
    for d in ["/usr/share/hunspell", "/usr/share/myspell", "/usr/share/myspell/dicts", "/usr/local/share/hunspell"] {
        v.push(PathBuf::from(d));
    }
    v
}

/// Languages with both files present, e.g. `en_US`, `it_IT`.
pub fn available(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "dic") && p.with_extension("aff").is_file() {
                let lang = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if !out.iter().any(|(l, _)| *l == lang) && !lang.starts_with("hyph") {
                    out.push((lang, p));
                }
            }
        }
    }
    out.sort();
    out
}

fn norm(l: &str) -> String {
    l.trim().replace('-', "_").to_lowercase()
}

/// `en_US`, `en-us` and `en` all pick `en_US.dic`.
fn find<'a>(avail: &'a [(String, PathBuf)], lang: &str) -> Option<&'a (String, PathBuf)> {
    let want = norm(lang);
    avail.iter().find(|(l, _)| norm(l) == want).or_else(|| avail.iter().find(|(l, _)| norm(l).starts_with(&format!("{want}_"))))
}

/// Hunspell files are UTF-8 or (older ones) ISO-8859-1/15.
fn decode(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect())
}

pub fn load_dictionary(dic: &Path) -> Result<Dictionary, String> {
    let aff = decode(std::fs::read(dic.with_extension("aff")).map_err(|e| e.to_string())?);
    let words = decode(std::fs::read(dic).map_err(|e| e.to_string())?);
    Dictionary::new(&aff, &words).map_err(|e| e.to_string())
}

/// Words of `text` worth checking: letters with inner apostrophes, outside code, math, links,
/// tags, frontmatter, URLs and the like. Byte ranges.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let mut skip: Vec<Range<usize>> = Vec::new();
    for b in parse_blocks(text) {
        match b.kind {
            BlockKind::Code { .. } | BlockKind::MathBlock | BlockKind::Frontmatter => skip.push(b.range.clone()),
            BlockKind::Blank | BlockKind::Rule => {}
            _ => {
                fn walk(n: &[Inline], skip: &mut Vec<Range<usize>>, text: &str) {
                    for i in n {
                        match i.kind {
                            InlineKind::Code | InlineKind::Math { .. } | InlineKind::Tag | InlineKind::BlockId | InlineKind::Image | InlineKind::Comment => skip.push(i.range.clone()),
                            InlineKind::WikiLink { embed } => {
                                // only a shown alias is prose
                                if embed || !text[i.range.clone()].contains('|') {
                                    skip.push(i.range.clone());
                                } else {
                                    skip.push(i.range.start..i.inner.start);
                                    skip.push(i.inner.end..i.range.end);
                                }
                            }
                            InlineKind::Link => {
                                skip.push(i.inner.end..i.range.end); // the URL
                                walk(&i.children, skip, text);
                            }
                            _ => walk(&i.children, skip, text),
                        }
                    }
                }
                walk(&parse_inline(text, b.content.clone()), &mut skip, text);
                // callout headers `[!type]`
                let head = &text[b.content.clone()];
                if let Some(s) = head.find("[!") {
                    if let Some(e) = head[s..].find(']') {
                        skip.push(b.content.start + s..b.content.start + s + e + 1);
                    }
                }
            }
        }
    }
    // HTML tags (<br>, <span …>, </div>)
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(p) = text[i..].find('<') {
        let at = i + p;
        let next = b.get(at + 1).copied().unwrap_or(b' ');
        if next.is_ascii_alphabetic() || next == b'/' {
            if let Some(e) = text[at + 1..].find(['>', '\n', '<']).map(|e| at + 1 + e).filter(|e| b[*e] == b'>') {
                skip.push(at..e + 1);
                i = e + 1;
                continue;
            }
        }
        i = at + 1;
    }
    // URLs
    let mut i = 0;
    while let Some(p) = text[i..].find("://") {
        let at = i + p;
        let s = text[..at].rfind(|c: char| !c.is_ascii_alphanumeric()).map_or(0, |x| x + 1);
        let e = text[at..].find(char::is_whitespace).map_or(text.len(), |x| at + x);
        skip.push(s..e);
        i = e.max(at + 3).min(text.len());
    }
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((s, c)) = it.next() {
        if !c.is_alphabetic() {
            continue;
        }
        let mut e = s + c.len_utf8();
        let mut has_digit = false;
        while let Some(&(j, d)) = it.peek() {
            let apostrophe = (d == '\'' || d == '’') && text[j + d.len_utf8()..].chars().next().is_some_and(char::is_alphabetic);
            if d.is_alphanumeric() || apostrophe || d == '_' {
                has_digit |= d.is_ascii_digit() || d == '_';
                e = j + d.len_utf8();
                it.next();
            } else {
                break;
            }
        }
        let w = s..e;
        if has_digit || skip.iter().any(|k| k.start < w.end && w.start < k.end) {
            continue;
        }
        out.push(w);
    }
    out
}

pub struct Checker {
    dicts: Arc<OnceLock<Vec<(String, Dictionary)>>>,
    /// Accept words in capitals (acronyms: TCP, ENDIF).
    ignore_caps: bool,
    /// Accept words with capitals inside (code names: makeSet, SonarQube).
    ignore_mixed: bool,
    custom: RefCell<HashSet<String>>,
    revision: Cell<u64>,
    words_file: PathBuf,
}

impl Checker {
    pub fn ready(&self) -> bool {
        self.dicts.get().is_some()
    }

    pub fn is_correct(&self, word: &str) -> bool {
        let Some(dicts) = self.dicts.get() else { return true };
        if dicts.is_empty() || word.chars().count() < 2 {
            return true;
        }
        let custom = self.custom.borrow();
        let skip_case = |w: &str| {
            let letters: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();
            let caps = letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase());
            let mixed = letters.iter().skip(1).any(|c| c.is_uppercase()) && letters.iter().any(|c| c.is_lowercase());
            (self.ignore_caps && caps) || (self.ignore_mixed && mixed)
        };
        let ok = |w: &str| !w.is_empty() && (skip_case(w.trim_matches('\'')) || custom.contains(w) || custom.contains(&w.to_lowercase()) || dicts.iter().any(|(_, d)| d.check(w)));
        let w = word.replace('’', "'");
        if ok(&w) {
            return true;
        }
        // elisions (l'albero, dell'anno, it's): accept when every part is a word
        if let Some((a, b)) = w.split_once('\'') {
            return (ok(&format!("{a}'")) || ok(a)) && (b.is_empty() || ok(b) || ok(&format!("'{b}")));
        }
        false
    }

    /// Suggestions, from the language the surrounding `context` is written in first; a
    /// suggestion differing only by accents (perchè → perché) leads.
    pub fn suggest(&self, word: &str, context: &str) -> Vec<String> {
        let Some(dicts) = self.dicts.get() else { return Vec::new() };
        // which dictionary knows most of the context's words (that the others don't)
        let ctx_words: Vec<&str> = words(context).into_iter().map(|r| &context[r]).collect();
        let mut order: Vec<(usize, usize)> = dicts
            .iter()
            .enumerate()
            .map(|(i, (_, d))| (ctx_words.iter().filter(|w| d.check(w) && !dicts.iter().enumerate().any(|(j, (_, o))| j != i && o.check(w))).count(), i))
            .collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let fold = |w: &str| -> String {
            w.to_lowercase()
                .chars()
                .map(|c| match c {
                    'à' | 'á' | 'â' | 'ä' => 'a',
                    'è' | 'é' | 'ê' | 'ë' => 'e',
                    'ì' | 'í' | 'î' | 'ï' => 'i',
                    'ò' | 'ó' | 'ô' | 'ö' => 'o',
                    'ù' | 'ú' | 'û' | 'ü' => 'u',
                    c => c,
                })
                .collect()
        };
        let mut out: Vec<String> = Vec::new();
        for (_, i) in order {
            let d = &dicts[i].1;
            let mut s = Vec::new();
            d.suggest(word, &mut s);
            for x in s {
                if !out.contains(&x) && x != word {
                    out.push(x);
                }
            }
        }
        let target = fold(word);
        out.sort_by_key(|x| fold(x) != target); // stable: accent fixes first
        out.truncate(6);
        out
    }

    /// Adds a word to the vault's dictionary (`.obsidilight/dictionary.txt`).
    pub fn add_word(&self, word: &str) {
        let w = word.trim().to_string();
        if w.is_empty() || !self.custom.borrow_mut().insert(w.clone()) {
            return;
        }
        self.revision.set(self.revision.get() + 1);
        let mut all: Vec<String> = self.custom.borrow().iter().cloned().collect();
        all.sort();
        if let Some(d) = self.words_file.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = self.words_file.with_extension("txt.obsidilight-tmp");
        if std::fs::write(&tmp, all.join("\n") + "\n").is_ok() {
            let _ = std::fs::rename(&tmp, &self.words_file);
        }
    }

    pub fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        words(text).into_iter().filter(|r| !self.is_correct(&text[r.clone()])).collect()
    }
}

struct Decorator(Rc<Checker>);

impl TextDecorator for Decorator {
    fn decorate(&self, block: &str) -> Vec<Range<usize>> {
        self.0.misspelled(block)
    }
    fn revision(&self) -> u64 {
        self.0.revision.get()
    }
    fn ready(&self) -> bool {
        self.0.ready()
    }
    fn color(&self) -> Hsla {
        gpui_kit::rgb(0xe93147).into()
    }
}

/// The word around `pos` in the editor (letters and inner apostrophes).
fn word_at(text: &str, pos: usize) -> Option<Range<usize>> {
    words(text).into_iter().find(|r| r.start <= pos && pos <= r.end)
}

pub struct SpellCheck {
    manifest: PluginManifest,
    vault: PathBuf,
}

impl SpellCheck {
    pub fn new(vault: &Path) -> Self {
        Self {
            manifest: PluginManifest {
                id: "spellcheck",
                name: "Spell check",
                version: "0.1.0",
                description: "Underlines misspelled words using Hunspell dictionaries; right-click for suggestions or to add a word.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "languages", label: "Languages", description: "Comma-separated, e.g. en_US, it_IT. A word is accepted if any language knows it.", kind: SettingKind::Text, default: Value::from("en_US, it_IT") },
                    SettingSpec { key: "ignoreUppercase", label: "Ignore words in capitals", description: "Acronyms like TCP or ENDIF are not flagged.", kind: SettingKind::Toggle, default: Value::Bool(true) },
                    SettingSpec { key: "ignoreMixedCase", label: "Ignore mixed-case words", description: "Code names like makeSet or SonarQube are not flagged.", kind: SettingKind::Toggle, default: Value::Bool(true) },
                    SettingSpec { key: "dictionaryFolders", label: "Extra dictionary folders", description: "Comma-separated folders with .aff/.dic files (system ones are found automatically).", kind: SettingKind::Text, default: Value::from("") },
                ],
                code_languages: &[],
            },
            vault: vault.to_path_buf(),
        }
    }
}

impl Plugin for SpellCheck {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str| settings.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let dirs = search_dirs(&self.vault, &get("dictionaryFolders"));
        let avail = available(&dirs);
        let langs: Vec<String> = get("languages").split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        let chosen: Vec<(String, PathBuf)> = langs.iter().filter_map(|l| find(&avail, l).cloned()).collect();
        let missing: Vec<String> = langs.iter().filter(|l| find(&avail, l).is_none()).cloned().collect();
        let words_file = self.vault.join(".obsidilight/dictionary.txt");
        let custom: HashSet<String> = std::fs::read_to_string(&words_file).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
        let dicts: Arc<OnceLock<Vec<(String, Dictionary)>>> = Arc::new(OnceLock::new());
        let errors: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        {
            // load off the UI thread (a big dictionary takes a moment to parse)
            let dicts = dicts.clone();
            let errors = errors.clone();
            std::thread::spawn(move || {
                let mut v = Vec::new();
                for (lang, path) in chosen {
                    match load_dictionary(&path) {
                        Ok(d) => v.push((lang, d)),
                        Err(e) => errors.lock().map(|mut x| x.push(format!("{lang}: {e}"))).unwrap_or(()),
                    }
                }
                let _ = dicts.set(v);
            });
        }
        let flag = |k: &str| settings.get(k).and_then(Value::as_bool).unwrap_or(true);
        let checker = Rc::new(Checker { dicts: dicts.clone(), ignore_caps: flag("ignoreUppercase"), ignore_mixed: flag("ignoreMixedCase"), custom: RefCell::new(custom), revision: Cell::new(0), words_file });
        reg.text_decorator(Rc::new(Decorator(checker.clone())));

        let c = checker.clone();
        reg.editor_menu(Rc::new(move |e: &Editor, _: &App| {
            let sel = e.doc.selection();
            // a selection: add it as is; otherwise the word at the caret
            let (range, word) = if !sel.is_empty() {
                let r = sel.range();
                (r.clone(), e.doc.slice(r))
            } else {
                let line = e.doc.line_of(sel.head);
                let ls = e.doc.line_start(line);
                let text = e.doc.line_text(line);
                let Some(r) = word_at(&text, sel.head - ls) else { return Vec::new() };
                (ls + r.start..ls + r.end, text[r].to_string())
            };
            let word = word.trim().to_string();
            if word.is_empty() || word.contains('\n') || word.chars().count() > 60 {
                return Vec::new();
            }
            let mut out = Vec::new();
            if sel.is_empty() && !c.is_correct(&word) {
                let line = e.doc.line_text(e.doc.line_of(sel.head));
                for s in c.suggest(&word, &line) {
                    let range = range.clone();
                    let label = s.clone();
                    out.push(MenuEntry {
                        label,
                        run: Rc::new(move |a: &mut ActionCtx| {
                            if let Some(ed) = a.editor.clone() {
                                let s = s.clone();
                                let range = range.clone();
                                ed.update(a.cx, |e, cx| {
                                    e.edit(range.clone(), &s, EditKind::Other, cx);
                                    e.set_selection(Selection::caret(range.start + s.len()), false, cx);
                                });
                            }
                        }),
                    });
                }
            }
            if !sel.is_empty() || !c.is_correct(&word) {
                let c = c.clone();
                let w = word.clone();
                out.push(MenuEntry {
                    label: format!("Add \u{201c}{word}\u{201d} to dictionary"),
                    run: Rc::new(move |a: &mut ActionCtx| {
                        c.add_word(&w);
                        a.cx.refresh_windows();
                    }),
                });
            }
            out
        }));

        let names: Vec<String> = avail.iter().map(|(l, _)| l.clone()).collect();
        reg.settings_tab(SettingsTab {
            id: "status".into(),
            title: "Dictionaries".into(),
            render: Rc::new(move |_, _| {
                let loaded: Vec<String> = dicts.get().map(|v| v.iter().map(|(l, _)| l.clone()).collect()).unwrap_or_default();
                let mut lines = vec![if dicts.get().is_none() { "Loading dictionaries…".to_string() } else if loaded.is_empty() { "No dictionary loaded: spell check is off.".into() } else { format!("Checking with: {}", loaded.join(", ")) }];
                lines.push(if names.is_empty() { "No Hunspell dictionaries found on this system.".into() } else { format!("Available: {}", names.join(", ")) });
                if !missing.is_empty() {
                    lines.push(format!("Not found: {}. Install them with your package manager (e.g. hunspell-en_us, hunspell-it) or put <lang>.aff/.dic in .obsidilight/dictionaries.", missing.join(", ")));
                }
                lines.extend(errors.lock().map(|e| e.clone()).unwrap_or_default());
                div().flex().flex_col().gap_1().text_sm().children(lines).into_any_element()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny Hunspell dictionary (synthetic): "cat" with plural, "gatto" with plural.
    fn checker() -> Checker {
        let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\nSFX S Y 1\nSFX S 0 s .\nSFX I Y 1\nSFX I o i o\n";
        let en = Dictionary::new(aff, "2\ncat/S\nthe\n").unwrap();
        let it = Dictionary::new(aff, "3\ngatto/I\nl'\nalbero\n").unwrap();
        let dicts = Arc::new(OnceLock::new());
        let _ = dicts.set(vec![("en".to_string(), en), ("it".to_string(), it)]);
        let dir = std::env::temp_dir().join(format!("ol-spell-{}", std::process::id()));
        Checker { dicts, ignore_caps: true, ignore_mixed: true, custom: RefCell::new(HashSet::new()), revision: Cell::new(0), words_file: dir.join("dictionary.txt") }
    }

    #[test]
    fn words_skip_code_math_links_and_urls() {
        let t = "Teh `codez` $x+yy$ [[Linkk]] [[a|Aliass]] #tagg https://exampl.com word2 l'albero";
        let w: Vec<&str> = words(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(w, vec!["Teh", "Aliass", "l'albero"]);
        let t = "---\ntitel: x\n---\n```\nnott\n```\n$$\nfrakk\n$$\n> [!note] Titel";
        let w: Vec<&str> = words(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(w, vec!["Titel"]);
        let t = "riga<br>altra <span class=\"x\">testo</span> a < b";
        let w: Vec<&str> = words(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(w, vec!["riga", "altra", "testo", "a", "b"]);
    }

    #[test]
    fn any_language_accepts_and_custom_words_stick() {
        let c = checker();
        assert!(c.is_correct("cats") && c.is_correct("gatti") && c.is_correct("the"));
        assert!(c.is_correct("l'albero"), "elision: l' + albero");
        assert!(!c.is_correct("catz"));
        assert!(c.is_correct("TCP") && c.is_correct("makeSet") && c.is_correct("l'TCP"), "acronyms and code names");
        assert!(!c.is_correct("Catz"), "a capitalised word is still checked");
        assert_eq!(c.misspelled("the catz and gatti"), vec![4..8, 9..12]);
        assert!(c.suggest("catz", "").contains(&"cat".to_string()) || c.suggest("catz", "").contains(&"cats".to_string()));
        c.add_word("catz");
        assert!(c.is_correct("catz"));
        assert_eq!(c.revision.get(), 1);
        assert!(std::fs::read_to_string(&c.words_file).unwrap().contains("catz"));
        let _ = std::fs::remove_dir_all(c.words_file.parent().unwrap());
    }

    /// Spell-checks every fixture note with the system en_US + it_IT dictionaries and writes the
    /// flagged words (unique, with counts) to $OBSIDILIGHT_SPELL_REPORT. Run on demand:
    /// `OBSIDILIGHT_SPELL_REPORT=reports/spell.txt cargo test -p obsidilight fixture_spell -- --ignored`
    #[test]
    #[ignore]
    fn fixture_spell_report() {
        let out = std::env::var("OBSIDILIGHT_SPELL_REPORT").expect("set OBSIDILIGHT_SPELL_REPORT");
        let home = std::env::var("HOME").unwrap();
        let root = Path::new(&home).join("dev/vault-fixture");
        let t = std::time::Instant::now();
        let mut v = Vec::new();
        for l in ["en_US", "it_IT"] {
            let t1 = std::time::Instant::now();
            v.push((l.to_string(), load_dictionary(Path::new(&format!("/usr/share/hunspell/{l}.dic"))).unwrap()));
            eprintln!("load {l}: {} ms", t1.elapsed().as_millis());
        }
        let dicts = Arc::new(OnceLock::new());
        let _ = dicts.set(v);
        let c = Checker { dicts, ignore_caps: true, ignore_mixed: true, custom: RefCell::new(HashSet::new()), revision: Cell::new(0), words_file: std::env::temp_dir().join("unused.txt") };
        eprintln!("both loaded: {} ms", t.elapsed().as_millis());
        for w in ["perchè", "qualcosa", "efficente", "algoritmo", "propietà", "sopratutto", "recieve"] {
            let t = std::time::Instant::now();
            let ctx = if w == "recieve" { "we will recieve the packet" } else { "il grafo e la sua struttura" };
            eprintln!("SUGGEST {w}: ok={} {:?} ({} ms)", c.is_correct(w), c.suggest(w, ctx), t.elapsed().as_millis());
        }
        fn md(d: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if p.is_dir() { md(&p, out) } else if p.extension().is_some_and(|x| x == "md") { out.push(p) }
            }
        }
        let mut files = Vec::new();
        md(&root, &mut files);
        let mut flagged: std::collections::BTreeMap<String, usize> = Default::default();
        let mut accepted: std::collections::BTreeSet<String> = Default::default();
        let (mut total, mut bad) = (0usize, 0usize);
        let t2 = std::time::Instant::now();
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            for r in words(&text) {
                total += 1;
                let w = &text[r];
                if !c.is_correct(w) {
                    bad += 1;
                    *flagged.entry(w.to_string()).or_default() += 1;
                } else if !w.contains(['\'', '’']) {
                    accepted.insert(w.to_string());
                }
            }
        }
        let ms = t2.elapsed().as_millis();
        eprintln!("{} notes, {total} words checked in {ms} ms, {bad} flagged ({} unique)", files.len(), flagged.len());
        let mut list: Vec<(String, usize)> = flagged.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        std::fs::write(&out, list.iter().map(|(w, n)| format!("{n}\t{w}\n")).collect::<String>()).unwrap();
        std::fs::write(format!("{out}.accepted"), accepted.into_iter().collect::<Vec<_>>().join("\n") + "\n").unwrap();
    }

    /// With the system's en_US + it_IT installed: Italian text, elisions, mixed languages.
    #[test]
    fn system_italian_and_english() {
        let dir = Path::new("/usr/share/hunspell");
        let (Ok(en), Ok(it)) = (load_dictionary(&dir.join("en_US.dic")), load_dictionary(&dir.join("it_IT.dic"))) else {
            return eprintln!("system dictionaries absent: skipped");
        };
        let dicts = Arc::new(OnceLock::new());
        let _ = dicts.set(vec![("en_US".to_string(), en), ("it_IT".to_string(), it)]);
        let c = Checker { dicts, ignore_caps: true, ignore_mixed: true, custom: RefCell::new(HashSet::new()), revision: Cell::new(0), words_file: std::env::temp_dir().join("unused.txt") };
        let t = "Dell'albero l'algoritmo calcola il cammino minimo, perché è efficiente: the shortest path tree.";
        assert!(c.misspelled(t).is_empty(), "{:?}", c.misspelled(t).iter().map(|r| &t[r.clone()]).collect::<Vec<_>>());
        let t = "il grafo è connesso perchè l'algoritmo è efficente";
        let bad: Vec<&str> = c.misspelled(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(bad, vec!["perchè", "efficente"]);
        assert_eq!(c.suggest("perchè", t).first().map(String::as_str), Some("perché"));
        assert_eq!(c.suggest("efficente", t).first().map(String::as_str), Some("efficiente"));
        assert_eq!(c.suggest("recieve", "we recieve the packets").first().map(String::as_str), Some("receive"));
    }

    #[test]
    fn language_names_match_loosely() {
        let avail = vec![("en_US".to_string(), PathBuf::from("/x/en_US.dic")), ("it_IT".to_string(), PathBuf::from("/x/it_IT.dic"))];
        assert_eq!(find(&avail, "en-us").map(|x| x.0.as_str()), Some("en_US"));
        assert_eq!(find(&avail, "it").map(|x| x.0.as_str()), Some("it_IT"));
        assert!(find(&avail, "de_DE").is_none());
    }
}
