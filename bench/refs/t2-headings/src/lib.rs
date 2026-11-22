#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading { pub level: u8, pub text: String, pub line: usize, pub children: Vec<Heading> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed { pub frontmatter: Option<String>, pub body_offset: usize, pub headings: Vec<Heading> }

fn fence_of(l: &str) -> Option<(char, usize)> {
    let ind = l.len() - l.trim_start_matches(' ').len();
    if ind > 3 { return None; }
    let r = &l[ind..];
    let c = r.chars().next()?;
    if c != '`' && c != '~' { return None; }
    let n = r.chars().take_while(|&x| x == c).count();
    if n >= 3 { Some((c, n)) } else { None }
}

fn heading(l: &str) -> Option<(u8, String)> {
    let ind = l.len() - l.trim_start_matches(' ').len();
    if ind > 3 { return None; }
    let r = &l[ind..];
    let n = r.chars().take_while(|&c| c == '#').count();
    if n == 0 || n > 6 { return None; }
    let rest = &r[n..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) { return None; }
    let mut t = rest.trim().to_string();
    let stripped = t.trim_end_matches('#');
    if stripped.len() != t.len() && (stripped.is_empty() || stripped.ends_with(' ') || stripped.ends_with('\t')) {
        t = stripped.trim_end().to_string();
    }
    Some((n as u8, t))
}

pub fn parse(md: &str) -> Parsed {
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut off = 0;
    for raw in md.split('\n') { lines.push((off, raw.strip_suffix('\r').unwrap_or(raw))); off += raw.len() + 1; }
    let mut frontmatter = None; let mut body_offset = 0; let mut start = 0;
    if lines.first().map(|l| l.1) == Some("---") {
        if let Some(end) = lines.iter().skip(1).position(|l| l.1 == "---").map(|p| p + 1) {
            frontmatter = Some(lines[1..end].iter().map(|l| l.1).collect::<Vec<_>>().join("\n"));
            body_offset = (lines.get(end + 1).map(|l| l.0)).unwrap_or(md.len()).min(md.len());
            start = end + 1;
        }
    }
    let mut flat: Vec<Heading> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, &(_, l)) in lines.iter().enumerate().skip(start) {
        if let Some((c, n)) = fence {
            if let Some((c2, n2)) = fence_of(l) { if c2 == c && n2 >= n { fence = None; } }
            continue;
        }
        if let Some(f) = fence_of(l) { fence = Some(f); continue; }
        if let Some((level, text)) = heading(l) { flat.push(Heading { level, text, line: i, children: vec![] }); }
    }
    fn build(items: &mut std::iter::Peekable<std::vec::IntoIter<Heading>>, parent_level: u8) -> Vec<Heading> {
        let mut out = Vec::new();
        while let Some(h) = items.peek() {
            if h.level <= parent_level { break; }
            let mut h = items.next().unwrap();
            h.children = build(items, h.level);
            out.push(h);
        }
        out
    }
    let mut it = flat.into_iter().peekable();
    let mut headings = Vec::new();
    while it.peek().is_some() { headings.extend(build(&mut it, 0)); }
    Parsed { frontmatter, body_offset, headings }
}
