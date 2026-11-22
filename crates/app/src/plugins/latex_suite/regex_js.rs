//! Translates JavaScript regex syntax to the Rust `regex` crate's: ASCII `\d \w \b` (as in JS),
//! literal `{` / `}` where JS allows them, `[` inside classes. Lookaround and backreferences
//! have no Rust equivalent: those return Err and the trigger runs in QuickJS instead.

pub fn translate(src: &str, flags: &str) -> Result<String, String> {
    let mut out = String::with_capacity(src.len() + 16);
    let f: String = flags.chars().filter(|c| matches!(c, 'i' | 'm' | 's')).collect();
    if !f.is_empty() {
        out.push_str(&format!("(?{f})"));
    }
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false; // just after `[` or `[^`
    while i < chars.len() {
        let c = chars[i];
        let at_class_start = class_start;
        class_start = false;
        match c {
            '\\' => {
                let Some(&n) = chars.get(i + 1) else { return Err("trailing backslash".into()) };
                i += 2;
                match n {
                    'd' => out.push_str(if in_class { "0-9" } else { "[0-9]" }),
                    'w' => out.push_str(if in_class { "0-9A-Za-z_" } else { "[0-9A-Za-z_]" }),
                    'D' if !in_class => out.push_str("[^0-9]"),
                    'W' if !in_class => out.push_str("[^0-9A-Za-z_]"),
                    'D' | 'W' => return Err("negated class escape inside a class".into()),
                    'b' if in_class => out.push_str("\\x08"),
                    'b' => out.push_str("(?-u:\\b)"),
                    'B' => out.push_str("(?-u:\\B)"),
                    '1'..='9' | 'k' => return Err("backreference".into()),
                    '0' => out.push_str("\\x00"),
                    'c' => return Err("control escape".into()),
                    'u' if chars.get(i) == Some(&'{') => out.push_str("\\u"),
                    's' | 'S' | 'n' | 't' | 'r' | 'f' | 'v' | 'x' | 'u' => {
                        out.push('\\');
                        out.push(n);
                    }
                    other if other.is_ascii_alphanumeric() => return Err(format!("unknown escape \\{other}")),
                    other => push_literal(&mut out, other, in_class),
                }
                continue;
            }
            '[' if !in_class => {
                // `[^]` = any character, `[]` = nothing
                if chars.get(i + 1) == Some(&'^') && chars.get(i + 2) == Some(&']') {
                    out.push_str("[\\s\\S]");
                    i += 3;
                    continue;
                }
                if chars.get(i + 1) == Some(&']') {
                    out.push_str("[^\\s\\S]");
                    i += 2;
                    continue;
                }
                in_class = true;
                class_start = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '[' | '&' | '~' if in_class => {
                out.push('\\');
                out.push(c);
            }
            ']' if !in_class => out.push_str("\\]"),
            '-' if in_class && at_class_start => out.push_str("\\-"),
            '{' if !in_class => {
                // a quantifier `{n}`, `{n,}`, `{n,m}`; anything else is a literal brace in JS
                let rest: String = chars[i..].iter().take(24).collect();
                let is_quant = rest.find('}').is_some_and(|e| {
                    let body = &rest[1..e];
                    !body.is_empty() && body.split(',').count() <= 2 && body.split(',').next().is_some_and(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit())) && body.split(',').nth(1).is_none_or(|b| b.bytes().all(|b| b.is_ascii_digit()))
                });
                out.push_str(if is_quant { "{" } else { "\\{" });
            }
            '}' if !in_class => {
                // closing brace of a quantifier stays, a stray one is literal
                let open = out.rfind('{');
                let closes_quant = open.is_some_and(|o| !out[..o].ends_with('\\') && out[o + 1..].bytes().all(|b| b.is_ascii_digit() || b == b','));
                out.push_str(if closes_quant { "}" } else { "\\}" });
            }
            '(' if !in_class && chars.get(i + 1) == Some(&'?') => {
                match chars.get(i + 2) {
                    Some(':') => out.push_str("(?:"),
                    Some('<') if chars.get(i + 3).is_some_and(|c| c.is_ascii_alphabetic()) => out.push_str("(?<"),
                    _ => return Err("lookaround".into()),
                }
                i += if chars.get(i + 2) == Some(&':') { 3 } else { 3 };
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    Ok(out)
}

fn push_literal(out: &mut String, c: char, in_class: bool) {
    let meta = if in_class { "\\[]^-&~" } else { "\\.+*?()|[]{}^$#&-~" };
    if meta.contains(c) {
        out.push('\\');
    }
    out.push(c);
}

#[cfg(test)]
mod tests {
    use super::translate;
    use regex::Regex;

    fn ok(js: &str, hay: &str) -> bool {
        let t = translate(&format!("(?:{js})$"), "").unwrap();
        Regex::new(&t).unwrap_or_else(|e| panic!("{js} -> {t}: {e}")).is_match(hay)
    }

    #[test]
    fn js_syntax_compiles_with_js_meaning() {
        assert!(ok(r"([^\\\w])beg", " beg"));
        assert!(!ok(r"([^\\\w])beg", "\\beg"));
        assert!(ok(r"\\begin{", "\\begin{"));
        assert!(ok(r"\\[A-Za-z]{2,}", "\\al"));
        assert!(ok(r"x{2}", "xx") && !ok(r"x{2}", "x"));
        assert!(ok(r"\/", "/"));
        assert!(ok(r"(\d)\.", "3."));
        assert!(!ok(r"\d", "٣"), "JS \\d is ASCII only");
        assert!(ok(r"a}", "a}"));
        assert!(ok(r"[^]", "\n"));
        assert!(translate(r"(?<=a)b", "").is_err());
        assert!(translate(r"(a)\1", "").is_err());
        assert!(translate("x", "i").unwrap().starts_with("(?i)"));
    }
}
