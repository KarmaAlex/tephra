//! Full-text search: query syntax and matching.

use tephra_core::search::{fold, parse_query, search, Expr, FileHit, SearchDoc, Term};

fn doc(path: &str, text: &str, tags: &[&str]) -> SearchDoc {
    SearchDoc::new(path, text, tags.iter().map(|t| t.to_string()).collect())
}

fn paths(hits: &[FileHit]) -> Vec<&str> {
    hits.iter().map(|h| h.path.as_str()).collect()
}

fn run(q: &str, docs: &[SearchDoc]) -> Vec<FileHit> {
    let refs: Vec<&SearchDoc> = docs.iter().collect();
    match parse_query(q) {
        Some(e) => search(&e, &refs),
        None => Vec::new(),
    }
}

fn w(s: &str) -> Expr {
    Expr::Term(Term::Word(s.into()))
}

#[test]
fn fold_keeps_byte_offsets() {
    assert_eq!(fold("Caffè ÀÈ Δx"), "caffè àè δx");
    for s in ["İstanbul", "ẞ straße", "ΣΑΣ", "ǅ", "K"] {
        assert_eq!(fold(s).len(), s.len(), "{s}");
    }
    assert_eq!(fold("İx"), "İx", "İ lowercases to two chars: kept as is");
}

#[test]
fn parse_words_phrases_or_not_groups() {
    assert_eq!(parse_query(""), None);
    assert_eq!(parse_query("   "), None);
    assert_eq!(parse_query("Graph"), Some(w("graph")));
    assert_eq!(parse_query("a b"), Some(Expr::And(vec![w("a"), w("b")])));
    assert_eq!(parse_query("\"divide and conquer\" sort"), Some(Expr::And(vec![Expr::Term(Term::Phrase("divide and conquer".into())), w("sort")])));
    assert_eq!(parse_query("a b OR c"), Some(Expr::Or(vec![Expr::And(vec![w("a"), w("b")]), w("c")])));
    assert_eq!(parse_query("a -b"), Some(Expr::And(vec![w("a"), Expr::Not(Box::new(w("b")))])));
    assert_eq!(parse_query("a (b OR c)"), Some(Expr::And(vec![w("a"), Expr::Or(vec![w("b"), w("c")])])));
    assert_eq!(parse_query("-(b c)"), Some(Expr::Not(Box::new(Expr::And(vec![w("b"), w("c")])))));
    // lowercase "or" is a word, not the operator
    assert_eq!(parse_query("a or b"), Some(Expr::And(vec![w("a"), w("or"), w("b")])));
}

#[test]
fn parse_operators_and_leniency() {
    assert_eq!(parse_query("path:Algo"), Some(Expr::Term(Term::Path("algo".into()))));
    assert_eq!(parse_query("PATH:\"Spring term\""), Some(Expr::Term(Term::Path("spring term".into()))));
    assert_eq!(parse_query("file:sor"), Some(Expr::Term(Term::File("sor".into()))));
    assert_eq!(parse_query("tag:#Work/CS"), Some(Expr::Term(Term::Tag("work/cs".into()))));
    assert_eq!(parse_query("tag:exam"), Some(Expr::Term(Term::Tag("exam".into()))));
    assert_eq!(parse_query("content:x"), Some(Expr::Term(Term::Content("x".into()))));
    assert_eq!(parse_query("-path:other"), Some(Expr::Not(Box::new(Expr::Term(Term::Path("other".into()))))));
    // unknown operator: the whole token is a word
    assert_eq!(parse_query("http://x.org"), Some(w("http://x.org")));
    // empty operator value, lone '-', stray ')' and empty groups are ignored
    assert_eq!(parse_query("path: a"), Some(w("a")));
    assert_eq!(parse_query("- a )"), Some(w("a")));
    assert_eq!(parse_query("() a"), Some(w("a")));
    // unterminated quote / group run to the end
    assert_eq!(parse_query("\"open phrase"), Some(Expr::Term(Term::Phrase("open phrase".into()))));
    assert_eq!(parse_query("(a b"), Some(Expr::And(vec![w("a"), w("b")])));
    // OR at the edges is dropped
    assert_eq!(parse_query("OR a OR"), Some(w("a")));
}

fn vault() -> Vec<SearchDoc> {
    vec![
        doc("Work/Algorithms/Sorting.md", "# Sorting\nThe comparison sorting problem.\nUses divide and conquer.\n", &["work/cs"]),
        doc("Work/Theory.md", "Turing machines\nDecidability\n", &["work"]),
        doc("Other/Planning.md", "Planning the project.\nA tree of ideas, tree everywhere.\n", &[]),
        doc("b.md", "nothing here", &["uniform"]),
    ]
}

#[test]
fn matching_is_case_insensitive_and_and_by_default() {
    let v = vault();
    assert_eq!(paths(&run("tree", &v)), vec!["Other/Planning.md"]);
    assert_eq!(paths(&run("TREE project", &v)), vec!["Other/Planning.md"]);
    assert_eq!(paths(&run("decidability", &v)), vec!["Work/Theory.md"]);
    assert_eq!(paths(&run("sorting", &v)), vec!["Work/Algorithms/Sorting.md"]);
    assert!(run("tree turing", &v).is_empty());
}

#[test]
fn results_sorted_by_file_name_then_path() {
    let v = vec![doc("z/beta.md", "x", &[]), doc("a/Gamma.md", "x", &[]), doc("y/alpha.md", "x", &[]), doc("b/alpha.md", "x", &[])];
    assert_eq!(paths(&run("x", &v)), vec!["b/alpha.md", "y/alpha.md", "z/beta.md", "a/Gamma.md"]);
}

#[test]
fn words_also_match_the_path() {
    let v = vault();
    // "planning" is in Planning.md's text and path; "other" only in its path
    assert_eq!(paths(&run("other", &v)), vec!["Other/Planning.md"]);
    let hits = run("other", &v);
    assert!(hits[0].lines.is_empty(), "a path-only match lists no lines");
}

#[test]
fn phrases_or_not_and_operators() {
    let v = vault();
    assert_eq!(paths(&run("\"divide and conquer\"", &v)), vec!["Work/Algorithms/Sorting.md"]);
    assert!(run("\"conquer divide\"", &v).is_empty());
    assert_eq!(paths(&run("turing OR sorting", &v)), vec!["Work/Algorithms/Sorting.md", "Work/Theory.md"]);
    assert_eq!(paths(&run("tree -sorting", &v)), vec!["Other/Planning.md"]);
    assert_eq!(paths(&run("tree path:other", &v)), vec!["Other/Planning.md"]);
    assert_eq!(paths(&run("file:sor", &v)), vec!["Work/Algorithms/Sorting.md"]);
    assert!(run("file:algorithms", &v).is_empty(), "file: is the file name only");
    assert_eq!(paths(&run("content:other", &v)), Vec::<&str>::new());
    assert_eq!(paths(&run("-path:work", &v)), vec!["b.md", "Other/Planning.md"]);
}

#[test]
fn tags_match_exactly_or_as_parent() {
    let v = vault();
    assert_eq!(paths(&run("tag:#work", &v)), vec!["Work/Algorithms/Sorting.md", "Work/Theory.md"]);
    assert_eq!(paths(&run("tag:work/cs", &v)), vec!["Work/Algorithms/Sorting.md"]);
    assert!(run("tag:cs", &v).is_empty(), "only whole leading segments match");
    assert_eq!(paths(&run("tag:UNIFORM", &v)), vec!["b.md"]);
}

#[test]
fn line_hits_with_merged_ranges() {
    let text = "# Sorting\nA tree of cases, TREE and treetop.\nUses divide and conquer.\n";
    let v = vec![doc("n.md", text, &[])];
    let hits = run("tree OR conquer", &v);
    assert_eq!(hits.len(), 1);
    let lines: Vec<(usize, usize, Vec<&str>)> = hits[0].lines.iter().map(|l| (l.line, l.start, l.ranges.iter().map(|r| &text[r.clone()]).collect())).collect();
    assert_eq!(
        lines,
        vec![
            (1, 10, vec!["tree", "TREE", "tree"]),
            (2, 45, vec!["conquer"]),
        ]
    );
    // overlapping matches of different terms merge into one range
    let v = vec![doc("n.md", "divide and conquer", &[])];
    let hits = run("\"divide and conquer\" conquer", &v);
    assert_eq!(hits[0].lines[0].ranges, vec![0..18]);
    // negated terms are not highlighted; a file matched only through tag/path lists no lines
    let v = vec![doc("n.md", "alpha beta\ngamma", &["t"])];
    let hits = run("alpha -gamma", &v);
    assert!(hits.is_empty());
    let hits = run("tag:t", &v);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].lines.is_empty());
    let hits = run("beta -delta", &v);
    assert_eq!(hits[0].lines.len(), 1);
    assert_eq!(hits[0].lines[0].ranges, vec![6..10]);
}

#[test]
fn crlf_lines_and_last_line_without_newline() {
    let text = "one\r\ntwo red\r\nred";
    let v = vec![doc("n.md", text, &[])];
    let hits = run("red", &v);
    let lines: Vec<(usize, usize)> = hits[0].lines.iter().map(|l| (l.line, l.start)).collect();
    assert_eq!(lines, vec![(1, 5), (2, 14)]);
    assert_eq!(&text[hits[0].lines[0].ranges[0].clone()], "red");
}

#[test]
fn large_vault_is_fast() {
    // ~1.2 MB of text; one query must stay far below the 100 ms budget
    let para = "Pack my box with five dozen liquor jugs, and a few more words to pad the line. ";
    let docs: Vec<SearchDoc> = (0..240).map(|i| doc(&format!("d{i}/n{i}.md"), &para.repeat(57), &[])).collect();
    let refs: Vec<&SearchDoc> = docs.iter().collect();
    let q = parse_query("box liquor -zebra").unwrap();
    let t = std::time::Instant::now();
    let hits = search(&q, &refs);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(hits.len(), 240);
    assert!(ms < if cfg!(debug_assertions) { 500.0 } else { 50.0 }, "search took {ms:.1} ms");
}
