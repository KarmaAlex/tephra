//! Pseudocode in `[!algorithm]` callouts: LaTeX `algorithmic` commands (`\STATE`, `\IF{…}`,
//! `\FOR{…}`, `\ENDFOR`, `\RETURN`, `\PROCEDURE{Name}{args}`…) turned into indented lines with
//! keywords, as Academic Notes shows them. Reimplemented from its rendered output.

/// A piece of a pseudocode line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Keyword(String),
    /// Markdown (may contain `$math$`).
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub depth: usize,
    pub parts: Vec<Part>,
}

/// `{a}{b}` arguments at the start of `s` (balanced braces) and the rest.
fn args(s: &str, n: usize) -> (Vec<String>, &str) {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    for _ in 0..n {
        if !rest.starts_with('{') {
            break;
        }
        let mut depth = 0;
        let mut end = None;
        for (i, c) in rest.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(e) = end else { break };
        out.push(rest[1..e].to_string());
        rest = rest[e + 1..].trim_start();
    }
    (out, rest)
}

/// Parses the body lines of an algorithm callout (quote prefixes already removed).
pub fn parse(body: &[&str]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut depth = 0usize;
    let kw = |k: &str| Part::Keyword(k.to_string());
    let text = |t: &str| Part::Text(t.trim().to_string());
    for raw in body {
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        let Some(cmd_end) = l.strip_prefix('\\').map(|r| r.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(r.len()) + 1) else {
            // not a command: continues the previous line (or stands alone)
            match out.last_mut() {
                Some(prev) => prev.parts.push(text(l)),
                None => out.push(Line { depth, parts: vec![text(l)] }),
            }
            continue;
        };
        let cmd = &l[1..cmd_end];
        let rest = &l[cmd_end..];
        let mut push = |d: usize, parts: Vec<Part>| out.push(Line { depth: d, parts: parts.into_iter().filter(|p| !matches!(p, Part::Text(t) if t.is_empty())).collect() });
        match cmd.to_uppercase().as_str() {
            "STATE" | "STATEX" => push(depth, vec![text(rest)]),
            "RETURN" => push(depth, vec![kw("return"), text(rest)]),
            "PRINT" => push(depth, vec![kw("print"), text(rest)]),
            "REQUIRE" => push(depth, vec![kw("Require:"), text(rest)]),
            "ENSURE" => push(depth, vec![kw("Ensure:"), text(rest)]),
            "COMMENT" => {
                let (a, r) = args(rest, 1);
                push(depth, vec![Part::Text(format!("▷ {}", a.first().map_or(r, |s| s.as_str())))]);
            }
            "IF" => {
                let (a, r) = args(rest, 1);
                push(depth, vec![kw("if"), text(a.first().map_or(r, |s| s.as_str())), kw("then")]);
                depth += 1;
            }
            "ELSIF" | "ELSEIF" => {
                let (a, r) = args(rest, 1);
                push(depth.saturating_sub(1), vec![kw("else if"), text(a.first().map_or(r, |s| s.as_str())), kw("then")]);
            }
            "ELSE" => push(depth.saturating_sub(1), vec![kw("else"), text(rest)]),
            "FOR" | "FORALL" => {
                let (a, r) = args(rest, 1);
                let k = if cmd.eq_ignore_ascii_case("FORALL") { "for all" } else { "for" };
                push(depth, vec![kw(k), text(a.first().map_or(r, |s| s.as_str())), kw("do")]);
                depth += 1;
            }
            "WHILE" => {
                let (a, r) = args(rest, 1);
                push(depth, vec![kw("while"), text(a.first().map_or(r, |s| s.as_str())), kw("do")]);
                depth += 1;
            }
            "LOOP" => {
                push(depth, vec![kw("loop")]);
                depth += 1;
            }
            "REPEAT" => {
                push(depth, vec![kw("repeat")]);
                depth += 1;
            }
            "UNTIL" => {
                depth = depth.saturating_sub(1);
                let (a, r) = args(rest, 1);
                push(depth, vec![kw("until"), text(a.first().map_or(r, |s| s.as_str()))]);
            }
            "PROCEDURE" | "FUNCTION" => {
                let (a, _) = args(rest, 2);
                let name = a.first().cloned().unwrap_or_default();
                let params = a.get(1).cloned().unwrap_or_default();
                push(depth, vec![kw(&cmd.to_lowercase()), Part::Text(format!("{name}({params})"))]);
                depth += 1;
            }
            c if c.starts_with("END") && c.len() > 3 => {
                depth = depth.saturating_sub(1);
                let what = c[3..].to_lowercase();
                push(depth, vec![kw(&format!("end {what}"))]);
            }
            _ => push(depth, vec![text(l)]), // other TeX (e.g. a formula line)
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                let p: Vec<String> = l.parts.iter().map(|p| match p { Part::Keyword(k) => format!("*{k}*"), Part::Text(t) => t.clone() }).collect();
                format!("{}{}", "  ".repeat(l.depth), p.join(" "))
            })
            .collect()
    }

    #[test]
    fn algorithmic_commands() {
        let body = [r"\STATE Let $x_1$ be any vertex", r"\FOR {every $i$ from $2$ to $k$}", r"\STATE pick $x_i$", r"\ENDFOR", r"\RETURN $X$"];
        assert_eq!(show(&parse(&body)), vec!["Let $x_1$ be any vertex", "*for* every $i$ from $2$ to $k$ *do*", "  pick $x_i$", "*end for*", "*return* $X$"]);
        let body = [r"\PROCEDURE{Node}{${}v{}$: integer}", r"\IF{${}W[0]=1{}$}", r"\STATE a", r"\ELSE", r"\STATE b", r"\ENDIF", r"\WHILE{true}", r"\STATE c", r"\ENDWHILE", r"\ENDPROCEDURE"];
        assert_eq!(
            show(&parse(&body)),
            vec!["*procedure* Node(${}v{}$: integer)", "  *if* ${}W[0]=1{}$ *then*", "    a", "  *else*", "    b", "  *end if*", "  *while* true *do*", "    c", "  *end while*", "*end procedure*"]
        );
    }
}
