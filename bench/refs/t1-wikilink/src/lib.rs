pub fn resolve_link(link: &str, files: &[&str], from: &str) -> Option<String> {
    let mut t = link.trim();
    if let Some(s) = t.strip_prefix('!') { if s.starts_with("[[") { t = s; } }
    if let Some(s) = t.strip_prefix("[[") { t = s.strip_suffix("]]").unwrap_or(s); }
    let t = t.split('|').next().unwrap_or("");
    let t = t.split('#').next().unwrap_or("").trim();
    if t.is_empty() { return None; }
    let last = t.rsplit('/').next().unwrap_or(t);
    let target = if last.contains('.') { t.to_string() } else { format!("{t}.md") };
    let tl = target.to_lowercase();
    if let Some(f) = files.iter().find(|f| f.to_lowercase() == tl) { return Some(f.to_string()); }
    let suffix = format!("/{tl}");
    let mut cands: Vec<&str> = files.iter().copied().filter(|f| f.to_lowercase().ends_with(&suffix)).collect();
    if cands.is_empty() { return None; }
    let dir = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let from_dir = dir(from);
    if let Some(c) = cands.iter().filter(|c| dir(c) == from_dir).min() { return Some(c.to_string()); }
    cands.sort_by(|a, b| a.split('/').count().cmp(&b.split('/').count()).then(a.cmp(b)));
    Some(cands[0].to_string())
}
