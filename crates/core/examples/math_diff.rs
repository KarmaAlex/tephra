//! Debug helper: formulas found by the whole-document scan but not by the per-block scan.
use tephra_core::markdown::*;
fn collect(v: &[Inline], out: &mut Vec<std::ops::Range<usize>>) {
    for n in v { if matches!(n.kind, InlineKind::Math { .. }) { out.push(n.range.clone()); } collect(&n.children, out); }
}
fn main() {
    let p = std::env::args().nth(1).expect("note path");
    let t = std::fs::read_to_string(p).unwrap();
    let blocks = parse_blocks(&t);
    let mut got = Vec::new();
    for b in &blocks {
        match b.kind {
            BlockKind::MathBlock => got.push(b.range.clone()),
            BlockKind::Code { .. } | BlockKind::Frontmatter | BlockKind::Blank | BlockKind::Rule => {}
            _ => collect(&parse_inline(&t, b.content.clone()), &mut got),
        }
    }
    for m in find_math(&t) {
        let covered = got.iter().any(|g| g.start <= m.range.start && m.range.end <= g.end);
        if !covered {
            let line = t[..m.range.start].matches('\n').count() + 1;
            let kinds: Vec<_> = blocks.iter().filter(|b| b.range.start < m.range.end && m.range.start < b.range.end).map(|b| format!("{:?}", b.kind)).collect();
            let snip: String = t[m.range.clone()].chars().take(60).collect();
            println!("line {line} display={} blocks={kinds:?} :: {:?}", m.display, snip);
        }
    }
}
