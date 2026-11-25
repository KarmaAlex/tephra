//! Where the caret is (Latex Suite's `Context`): text / inline math / display math / code, the
//! equation bounds, the text-environment state (`\text{…}`) and the enclosing environments.

use std::ops::Range;

use tephra_core::markdown::{parse_blocks, parse_inline, BlockKind, InlineKind};

use super::tex::{stack_at, Frame};
use crate::editor::api::math_at;

/// A macro argument list entry: `name` with the 0-based argument indices it applies to
/// (None = every argument).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroArgs {
    pub name: String,
    pub args: Option<Vec<usize>>,
    /// Restricted (snippetless) rather than text.
    pub restricted: bool,
}

/// Latex Suite's built-in text macros and restricted macros.
pub fn default_mathless_macros() -> Vec<MacroArgs> {
    let t = |n: &str, a: usize| MacroArgs { name: n.into(), args: Some(vec![a]), restricted: false };
    let r = |n: &str, a: &[usize]| MacroArgs { name: n.into(), args: Some(a.to_vec()), restricted: true };
    let mut v: Vec<MacroArgs> = ["text", "textrm", "textup", "textit", "textbf", "textsf", "texttt", "textnormal", "clap", "textllap", "textrlap", "textclap", "hbox", "mbox", "fbox", "framebox"]
        .iter()
        .map(|n| t(n, 0))
        .collect();
    v.push(t("colorbox", 1));
    v.push(t("fcolorbox", 2));
    v.extend([
        r("tag", &[0]),
        r("begin", &[0]),
        r("end", &[0]),
        r("mmlToken", &[0, 1]),
        r("unicode", &[0]),
        r("textcolor", &[0]),
        r("color", &[0]),
        r("colorbox", &[0]),
        r("fcolorbox", &[0, 1]),
        r("style", &[0]),
    ]);
    v
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub text: bool,
    pub inline_math: bool,
    pub block_math: bool,
    /// A code block whose language forces math (`math`).
    pub code_math: bool,
    /// Inside a fenced code block: its language (possibly empty).
    pub code_block: Option<String>,
    /// Inline code.
    pub code: bool,
    pub text_env: bool,
    pub snippetless_env: bool,
}

impl Mode {
    pub fn in_math(&self) -> bool {
        self.inline_math || self.block_math || self.code_math
    }
    pub fn strictly_in_math(&self) -> bool {
        self.in_math() && !self.text_env
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub inner_start: usize,
    pub inner_end: usize,
    pub outer_start: usize,
    pub outer_end: usize,
}

/// An enclosing environment or command argument, innermost first (Latex Suite's `getEnvNames`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvName {
    Environment { name: String, bounds: Bounds },
    Command { name: String, arg: usize },
    Math,
}

pub struct Ctx {
    pub pos: usize,
    pub mode: Mode,
    pub bounds: Option<Bounds>,
    /// Innermost first; ends with `Math` when in math.
    pub envs: Vec<EnvName>,
}

impl Ctx {
    pub fn new(text: &str, pos: usize, force_math_languages: &[String], mathless: &[MacroArgs]) -> Self {
        let mut mode = Mode::default();
        let mut bounds = None;
        let block = parse_blocks(text).into_iter().find(|b| b.range.start <= pos && pos <= b.range.end);
        let mut in_code_block = false;
        if let Some(b) = &block {
            if let BlockKind::Code { lang, .. } = &b.kind {
                if b.content.start <= pos && pos <= b.content.end {
                    in_code_block = true;
                    if force_math_languages.iter().any(|l| l.eq_ignore_ascii_case(lang)) {
                        mode.code_math = true;
                        bounds = Some(Bounds { inner_start: b.content.start, inner_end: b.content.end, outer_start: b.content.start, outer_end: b.content.end });
                    } else {
                        mode.code_block = Some(lang.clone());
                    }
                }
            }
        }
        if !in_code_block {
            if let Some(m) = math_at(text, pos) {
                if m.display {
                    mode.block_math = true;
                } else {
                    mode.inline_math = true;
                }
                bounds = Some(Bounds { inner_start: m.inner.start, inner_end: m.inner.end, outer_start: m.outer.start, outer_end: m.outer.end });
            } else if let Some(b) = &block {
                mode.code = in_inline_code(text, b.range.clone(), pos);
            }
        }
        let mut envs = Vec::new();
        if let Some(bd) = bounds {
            let tex = &text[bd.inner_start..bd.inner_end];
            let rel = pos - bd.inner_start;
            for f in stack_at(tex, rel).into_iter().rev() {
                match f {
                    Frame::Env { name, start } => {
                        let end = env_end(tex, start, &name).unwrap_or(tex.len());
                        let b = Bounds { inner_start: bd.inner_start + start, inner_end: bd.inner_start + end, outer_start: bd.inner_start + start, outer_end: bd.inner_start + end };
                        envs.push(EnvName::Environment { name, bounds: b });
                    }
                    Frame::Cmd { name, arg, .. } => envs.push(EnvName::Command { name, arg }),
                    Frame::Group { .. } => {}
                }
            }
            envs.push(EnvName::Math);
            match within_macros(&envs, mathless) {
                Some(true) => mode.snippetless_env = true,
                Some(false) => mode.text_env = true,
                None => {}
            }
        }
        mode.text = !in_code_block && bounds.is_none();
        Ctx { pos, mode, bounds, envs }
    }

    /// The innermost environment / command / math (matrix shortcuts look at this).
    pub fn innermost(&self) -> Option<&EnvName> {
        self.envs.first()
    }

    /// Latex Suite's `isWithinEnvironment` for `(open, close)` delimiters like `("^{", "}")`.
    pub fn is_within(&self, text: &str, open: &str, close: &str) -> bool {
        let Some(b) = self.bounds.filter(|_| self.mode.in_math()) else { return false };
        let tex = &text[b.inner_start..b.inner_end];
        let pos = self.pos - b.inner_start;
        let open_bracket = open.chars().last().unwrap_or(' ');
        let (offset, search_open) = if matches!(open_bracket, '{' | '[' | '(') { (open.len() - 1, open_bracket.to_string()) } else { (0, open.to_string()) };
        let mut search_to = pos;
        while let Some(left) = tex[..search_to.min(tex.len())].rfind(open) {
            let Some(right) = super::tex::find_matching_bracket(tex, left + offset, &search_open, close, false) else { return false };
            if right >= pos && pos >= left + open.len() {
                return true;
            }
            if left == 0 {
                return false;
            }
            search_to = left + open.len() - 1;
        }
        false
    }
}

/// End (after `\end{name}`) of the environment whose `\begin` is at `start`, if closed.
fn env_end(tex: &str, start: usize, name: &str) -> Option<usize> {
    let begin = format!("\\begin{{{name}}}");
    let end = format!("\\end{{{name}}}");
    let mut depth = 0;
    let mut i = start;
    while i < tex.len() {
        if tex[i..].starts_with(&begin) {
            depth += 1;
            i += begin.len();
        } else if tex[i..].starts_with(&end) {
            depth -= 1;
            i += end.len();
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += tex[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// Some(restricted) when the caret is inside a text/restricted macro argument (environments
/// are looked through, math ends the search).
fn within_macros(envs: &[EnvName], macros: &[MacroArgs]) -> Option<bool> {
    for e in envs {
        match e {
            EnvName::Environment { .. } => continue,
            EnvName::Math => return None,
            EnvName::Command { name, arg } => {
                let cands: Vec<&MacroArgs> = macros.iter().filter(|m| m.name == *name).collect();
                if let Some(m) = cands.iter().find(|m| m.args.as_ref().is_none_or(|a| a.contains(arg))) {
                    return Some(m.restricted);
                }
            }
        }
    }
    None
}

fn in_inline_code(text: &str, block: Range<usize>, pos: usize) -> bool {
    fn walk(n: &[tephra_core::markdown::Inline], pos: usize) -> bool {
        n.iter().any(|i| (i.kind == InlineKind::Code && i.range.start < pos && pos < i.range.end) || walk(&i.children, pos))
    }
    walk(&parse_inline(text, block), pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(t: &str) -> Ctx {
        let pos = t.find('|').unwrap();
        let text = t.replacen('|', "", 1);
        Ctx::new(&text, pos, &["math".to_string()], &default_mathless_macros())
    }

    #[test]
    fn modes() {
        assert!(ctx("ab |c").mode.text);
        let c = ctx("a $x|$ b");
        assert!(c.mode.inline_math && !c.mode.text && c.mode.strictly_in_math());
        assert!(ctx("$$\nx|\n$$").mode.block_math);
        assert!(ctx("a `co|de` b").mode.code);
        assert_eq!(ctx("```py\nx|\n```").mode.code_block.as_deref(), Some("py"));
        assert!(ctx("```math\nx|\n```").mode.code_math);
        let c = ctx(r"$\text{hi |} x$");
        assert!(c.mode.text_env && !c.mode.strictly_in_math());
        assert!(ctx(r"$\color{re|d} x$").mode.snippetless_env);
    }

    #[test]
    fn environments_and_within() {
        let t = "$$\n\\begin{pmatrix}\na & b|\n\\end{pmatrix}\n$$";
        let c = ctx(t);
        assert!(matches!(c.innermost(), Some(EnvName::Environment { name, .. }) if name == "pmatrix"));
        let c = ctx(r"$x^{a|}$");
        let text = r"$x^{a}$";
        assert!(c.is_within(text, "^{", "}"));
        assert!(!c.is_within(text, "\\pu{", "}"));
    }
}
