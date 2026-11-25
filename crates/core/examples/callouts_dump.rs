//! Debug helper: every callout the index finds, as "file:line" (1-based).
fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("vault"));
    let idx = tephra_core::index::VaultIndex::build(&root);
    let mut v: Vec<String> = idx.notes().flat_map(|n| n.callouts.iter().map(move |c| format!("{}:{}", n.path, c.line + 1))).collect();
    v.sort();
    for l in v { println!("{l}"); }
}
