//! Snippet definitions and matching (Latex Suite's `parse.ts`, `options.ts`, `snippets.ts`,
//! `run_snippets.ts`; MIT). Positions are byte offsets.

use std::ops::Range;

use regex::Regex;
use serde_json::Value as Json;

use super::context::Mode;
use super::regex_js;

/// Which code blocks a snippet runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeSel {
    No,
    Any,
    Lang(String),
}

/// A snippet's mode flags (`m n M t T c C`), Latex Suite's `Mode`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnipMode {
    pub text: bool,
    pub inline_math: bool,
    pub block_math: bool,
    pub code_math: bool,
    pub code_block: CodeSel,
    pub code: bool,
    pub text_env: bool,
    pub snippetless_env: bool,
    /// No mode letter: runs everywhere (except snippetless environments).
    pub all: bool,
}

impl SnipMode {
    pub fn from_source(opts: &str, language: Option<&str>) -> Self {
        let mut m = SnipMode { text: false, inline_math: false, block_math: false, code_math: false, code_block: CodeSel::No, code: false, text_env: false, snippetless_env: false, all: false };
        for c in opts.chars() {
            match c {
                'm' => {
                    m.block_math = true;
                    m.inline_math = true;
                }
                'n' => m.inline_math = true,
                'M' => m.block_math = true,
                't' => m.text = true,
                'T' => m.text_env = true,
                'c' => m.code_block = CodeSel::Any,
                'C' => m.code = true,
                _ => {}
            }
        }
        if let Some(l) = language {
            m.code_block = CodeSel::Lang(l.to_string());
        }
        if m.text_env && !(m.block_math || m.inline_math) {
            m.block_math = true;
            m.inline_math = true;
        }
        if !(m.text || m.inline_math || m.block_math || m.code_math || m.code_block != CodeSel::No || m.text_env || m.code) {
            // backwards compatibility: no mode letter = everywhere (Mode.invert())
            m = SnipMode { text: true, inline_math: true, block_math: true, code_math: true, code_block: CodeSel::Any, code: true, text_env: true, snippetless_env: true, all: true };
        }
        m
    }

    /// Latex Suite's `snippetShouldRunInMode`.
    pub fn runs_in(&self, m: &Mode) -> bool {
        if m.snippetless_env {
            return false;
        }
        if ((self.inline_math && m.inline_math) || (self.block_math && m.block_math) || ((self.inline_math || self.block_math) && m.code_math)) && (m.text_env == self.text_env || self.all) {
            return true;
        }
        if self.text && m.text {
            return true;
        }
        if let Some(lang) = &m.code_block {
            match &self.code_block {
                CodeSel::Any => return true,
                CodeSel::Lang(l) if l == lang => return true,
                _ => {}
            }
        }
        self.code && m.code
    }
}

#[derive(Debug)]
pub enum Trigger {
    Str(String),
    /// Compiled `(?:source)$`; None = runs in QuickJS (`source`, `flags` kept for that).
    Regex { re: Option<Regex>, source: String, flags: String },
    /// Visual: fires on its trigger key with a selection.
    Visual,
}

#[derive(Debug)]
pub enum Replacement {
    Text(String),
    /// Function replacement, by index in the snippets file; `needs_state` = reads `view.state`.
    Js { index: usize, needs_state: bool },
}

#[derive(Debug)]
pub struct Snippet {
    pub trigger: Trigger,
    pub replacement: Replacement,
    pub mode: SnipMode,
    pub automatic: bool,
    pub word_boundary: bool,
    pub undo_key: bool,
    pub priority: f64,
    /// Key that runs this (non-automatic) snippet; "" = the snippets trigger (Tab).
    pub trigger_key: String,
    pub description: String,
    pub excluded_envs: Vec<String>,
    /// Trigger length used for sorting (regex: source length, as Latex Suite).
    sort_len: usize,
}

/// Replaces every snippet variable (`${GREEK}`…) in a trigger.
fn insert_variables(s: &str, vars: &[(String, String)]) -> String {
    let mut s = s.to_string();
    for (k, v) in vars {
        s = s.replace(k.as_str(), v);
    }
    s
}

/// Builds snippets from the JSON descriptors produced by the JS loader. Returns the snippets
/// (sorted like Latex Suite: priority, then trigger length, both descending) and warnings.
pub fn build(descs: &[Json], vars: &[(String, String)]) -> (Vec<Snippet>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for d in descs {
        let index = d["i"].as_u64().unwrap_or(0) as usize;
        let opts = d["options"].as_str().unwrap_or("");
        let language = d["language"].as_str();
        let mode = SnipMode::from_source(opts, language);
        let automatic = opts.contains('A');
        let regex_opt = opts.contains('r');
        let mut visual = opts.contains('v');
        let replacement = if d["fn"].as_bool() == Some(true) {
            Replacement::Js { index, needs_state: d["needsState"].as_bool().unwrap_or(false) }
        } else if let Some(r) = d["replacement"].as_str() {
            Replacement::Text(r.to_string())
        } else {
            warnings.push(format!("snippet {index}: unsupported replacement (node arrays are not supported)"));
            continue;
        };
        let priority = d["priority"].as_f64().unwrap_or(0.0);
        let description = d["description"].as_str().unwrap_or("").to_string();
        let excluded_envs: Vec<String> = d["excludedEnvironments"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let trigger_key = d["triggerKey"].as_str().unwrap_or("").to_string();
        let base = |trigger, sort_len, automatic, trigger_key| Snippet {
            trigger,
            replacement: match &replacement {
                Replacement::Text(t) => Replacement::Text(t.clone()),
                Replacement::Js { index, needs_state } => Replacement::Js { index: *index, needs_state: *needs_state },
            },
            mode: mode.clone(),
            automatic,
            word_boundary: opts.contains('w'),
            undo_key: !opts.contains('U'),
            priority,
            trigger_key,
            description: description.clone(),
            excluded_envs: excluded_envs.clone(),
            sort_len,
        };
        if let Some(src) = d["regex"].as_str().map(String::from).or_else(|| regex_opt.then(|| d["trigger"].as_str().unwrap_or("").to_string())) {
            let flags = d["flags"].as_str().unwrap_or("").to_string();
            let src = insert_variables(&src, vars);
            let full = format!("(?:{src})$");
            let re = regex_js::translate(&full, &flags).ok().and_then(|t| Regex::new(&t).ok());
            out.push(base(Trigger::Regex { re, source: src, flags }, full.len(), automatic, trigger_key));
            continue;
        }
        let trigger = insert_variables(d["trigger"].as_str().unwrap_or(""), vars);
        if matches!(&replacement, Replacement::Text(r) if r.contains("${VISUAL}")) && trigger.chars().count() <= 1 {
            visual = true;
        }
        if visual {
            if trigger.chars().count() > 1 {
                warnings.push(format!("snippet {index}: visual snippets need a single-character trigger"));
                continue;
            }
            let key = if trigger.is_empty() { trigger_key } else { trigger };
            out.push(base(Trigger::Visual, 0, false, key));
            continue;
        }
        let len = trigger.len();
        out.push(base(Trigger::Str(trigger), len, automatic, trigger_key));
    }
    // stable sort: file order breaks ties
    out.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal).then(b.sort_len.cmp(&a.sort_len)));
    (out, warnings)
}

/// A matched trigger, before the replacement is computed.
pub struct Match {
    /// Start of the trigger in the document.
    pub start: usize,
    /// Capture groups (index 0 = whole match); for string snippets just the trigger.
    pub groups: Vec<Option<String>>,
}

impl Snippet {
    pub fn is_regex(&self) -> bool {
        matches!(self.trigger, Trigger::Regex { .. })
    }

    /// Matches against `line` (= document text before the caret + the typed key). `window` is
    /// where regex searching starts (earlier text is context for `^`/`\b` only). `js` runs
    /// regexes the Rust engine could not compile.
    pub fn match_at(&self, line: &str, window: usize, to: usize, js: Option<&super::js::Js>) -> Option<Match> {
        match &self.trigger {
            Trigger::Str(t) => {
                if t.is_empty() || !line.ends_with(t.as_str()) {
                    return None;
                }
                Some(Match { start: line.len() - t.len(), groups: vec![Some(t.clone())] })
            }
            Trigger::Regex { re: Some(re), .. } => {
                let caps = re.captures_at(line, window)?;
                let m = caps.get(0)?;
                if m.start() > to {
                    return None;
                }
                Some(Match { start: m.start(), groups: caps.iter().map(|g| g.map(|g| g.as_str().to_string())).collect() })
            }
            Trigger::Regex { re: None, source, flags } => {
                let hay = &line[window..];
                let (idx16, groups) = js?.exec(source, flags, hay)?;
                let start = window + utf16_to_byte(hay, idx16);
                (start <= to).then_some(Match { start, groups })
            }
            Trigger::Visual => None,
        }
    }
}

pub fn utf16_to_byte(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}

pub fn byte_to_utf16(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].encode_utf16().count()
}

/// A replacement after expansion: text and tabstops (group number, range in `text`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Insert {
    pub text: String,
    pub tabstops: Vec<(usize, Range<usize>)>,
}

/// `[[n]]` -> capture n (0 = first group), then `$n` / `${n:text}` -> tabstops.
pub fn expand(template: &str, captures: &[String], visual: Option<&str>) -> Insert {
    const VIS: &str = "\u{E000}";
    let mut s = template.to_string();
    if visual.is_some() {
        s = s.replace("${VISUAL}", VIS);
    }
    if !captures.is_empty() {
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(p) = rest.find("[[") {
            let after = &rest[p + 2..];
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && after[digits..].starts_with("]]") {
                let n: usize = after[..digits].parse().unwrap_or(usize::MAX);
                if n < captures.len() {
                    out.push_str(&rest[..p]);
                    out.push_str(&captures[n]);
                    rest = &after[digits + 2..];
                    continue;
                }
            }
            out.push_str(&rest[..p + 2]);
            rest = after;
        }
        out.push_str(rest);
        s = out;
    }
    let mut ins = expand_tabstops(&s);
    if let Some(sel) = visual {
        // substitute the selection after parsing, so `$1` in selected text stays text
        while let Some(p) = ins.text.find(VIS) {
            let delta = sel.len() as isize - VIS.len() as isize;
            ins.text.replace_range(p..p + VIS.len(), sel);
            for (_, r) in &mut ins.tabstops {
                let sh = |x: usize| if x > p { (x as isize + delta) as usize } else { x };
                *r = sh(r.start)..sh(r.end);
            }
        }
    }
    ins
}

/// `$n` (one digit) and `${n:placeholder}` -> tabstops.
pub fn expand_tabstops(s: &str) -> Insert {
    let mut text = String::with_capacity(s.len());
    let mut tabstops = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            if let Some(d) = b.get(i + 1).filter(|d| d.is_ascii_digit()) {
                let at = text.len();
                tabstops.push(((d - b'0') as usize, at..at));
                i += 2;
                continue;
            }
            if b.get(i + 1) == Some(&b'{') {
                let digits = b[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 && b.get(i + 2 + digits) == Some(&b':') {
                    let body_start = i + 3 + digits;
                    if let Some(close) = s[body_start..].find('}') {
                        let n: usize = s[i + 2..i + 2 + digits].parse().unwrap_or(0);
                        let body = &s[body_start..body_start + close];
                        let at = text.len();
                        text.push_str(body);
                        tabstops.push((n, at..at + body.len()));
                        i = body_start + close + 1;
                        continue;
                    }
                }
            }
        }
        let c = s[i..].chars().next().expect("boundary");
        text.push(c);
        i += c.len_utf8();
    }
    Insert { text, tabstops }
}

/// Inline math: drop trailing whitespace of a replacement, clamping tabstops.
pub fn trim_end(ins: &mut Insert) {
    let n = ins.text.trim_end().len();
    ins.text.truncate(n);
    for (_, r) in &mut ins.tabstops {
        *r = r.start.min(n)..r.end.min(n);
    }
}

/// Latex Suite's word-boundary check: the characters before the trigger and after the caret
/// are delimiters (document edges count as delimiters).
pub fn on_word_boundary(text: &str, trigger_pos: usize, to: usize, delimiters: &str) -> bool {
    let prev = text[..trigger_pos.min(text.len())].chars().next_back();
    let next = text[to.min(text.len())..].chars().next();
    prev.is_none_or(|c| delimiters.contains(c)) && next.is_none_or(|c| delimiters.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modes_follow_latex_suite() {
        let math = Mode { inline_math: true, ..Default::default() };
        let text = Mode { text: true, ..Default::default() };
        let text_env = Mode { inline_math: true, text_env: true, ..Default::default() };
        let m = SnipMode::from_source("mA", None);
        assert!(m.runs_in(&math) && !m.runs_in(&text) && !m.runs_in(&text_env));
        let t = SnipMode::from_source("tA", None);
        assert!(t.runs_in(&text) && !t.runs_in(&math));
        let tt = SnipMode::from_source("TA", None);
        assert!(tt.runs_in(&text_env) && !tt.runs_in(&math));
        let all = SnipMode::from_source("A", None);
        assert!(all.runs_in(&math) && all.runs_in(&text) && all.runs_in(&text_env));
        assert!(!all.runs_in(&Mode { inline_math: true, snippetless_env: true, ..Default::default() }));
        assert!(SnipMode::from_source("nA", None).runs_in(&math));
        assert!(!SnipMode::from_source("MA", None).runs_in(&math));
    }

    #[test]
    fn expansion() {
        assert_eq!(expand_tabstops("\\frac{$0}{$1}$2"), Insert { text: "\\frac{}{}".into(), tabstops: vec![(0, 6..6), (1, 8..8), (2, 9..9)] });
        assert_eq!(expand_tabstops("${}$0{}$").text, "${}{}$");
        let t = expand_tabstops("${0:f}(${1:x})");
        assert_eq!(t.text, "f(x)");
        assert_eq!(t.tabstops, vec![(0, 0..1), (1, 2..3)]);
        let caps = vec![" ".to_string()];
        assert_eq!(expand("[[0]]\\begin{$0}", &caps, None).text, " \\begin{}");
        assert_eq!(expand("[[1]]x", &caps, None).text, "[[1]]x", "out-of-range captures stay literal");
        let v = expand("\\underbrace{ ${VISUAL} }_{ $0 }", &[], Some("a$1b"));
        assert_eq!(v.text, "\\underbrace{ a$1b }_{  }");
        assert_eq!(v.tabstops, vec![(0, 22..22)]);
        let mut i = Insert { text: "\\alpha ".into(), tabstops: vec![(0, 7..7)] };
        trim_end(&mut i);
        assert_eq!((i.text.as_str(), i.tabstops[0].1.clone()), ("\\alpha", 6..6));
    }

    #[test]
    fn build_sorts_and_substitutes_variables() {
        let vars = vec![("${GREEK}".to_string(), "alpha|beta".to_string())];
        let d = vec![
            json!({"i":0,"trigger":"ab","replacement":"x","options":"mA","priority":0}),
            json!({"i":1,"regex":"\\\\(${GREEK})","replacement":"y","options":"rmA","priority":0}),
            json!({"i":2,"trigger":"abc","replacement":"z","options":"mA","priority":0}),
            json!({"i":3,"trigger":"U","replacement":"\\underbrace{ ${VISUAL} }","options":"mA","priority":0}),
            json!({"i":4,"trigger":"q","replacement":"w","options":"mA","priority":3}),
        ];
        let (s, w) = build(&d, &vars);
        assert!(w.is_empty());
        assert!(matches!(&s[0].trigger, Trigger::Str(t) if t == "q"), "priority first");
        assert!(matches!(&s[1].trigger, Trigger::Regex { re: Some(_), .. }), "then longer triggers (regex source)");
        let vis = s.iter().find(|x| matches!(x.trigger, Trigger::Visual)).unwrap();
        assert_eq!((vis.trigger_key.as_str(), vis.automatic), ("U", false));
        let re = &s[1];
        let m = re.match_at("x \\beta", 0, 7, None).unwrap();
        assert_eq!((m.start, m.groups[1].as_deref()), (2, Some("beta")));
    }

    #[test]
    fn word_boundary() {
        let d = "., +-\n\t:;!?\\/{}[]()=~$'\"|`<>*^%#@&";
        assert!(on_word_boundary("x dm", 2, 4, d));
        assert!(!on_word_boundary("xdm", 1, 3, d));
        assert!(on_word_boundary("dm", 0, 2, d));
        assert!(!on_word_boundary("dm a", 0, 1, d));
    }
}
