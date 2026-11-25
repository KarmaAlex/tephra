//! Full-text search (HANDOFF §5.4): query syntax and matching. Written before the
//! implementation (delegated leaf task, see docs/delegation-log.md).

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
    assert_eq!(fold("Calcolabilità ÀÈ Δx"), "calcolabilità àè δx");
    for s in ["İstanbul", "ẞ straße", "ΣΑΣ", "ǅ", "K"] {
        assert_eq!(fold(s).len(), s.len(), "{s}");
    }
    assert_eq!(fold("İx"), "İx", "İ lowercases to two chars: kept as is");
}

#[test]
fn parse_words_phrases_or_not_groups() {
    assert_eq!(parse_query(""), None);
    assert_eq!(parse_query("   "), None);
    assert_eq!(parse_query("Grafo"), Some(w("grafo")));
    assert_eq!(parse_query("a b"), Some(Expr::And(vec![w("a"), w("b")])));
    assert_eq!(parse_query("\"Minimum Spanning\" tree"), Some(Expr::And(vec![Expr::Term(Term::Phrase("minimum spanning".into())), w("tree")])));
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
    assert_eq!(parse_query("PATH:\"Primo semestre\""), Some(Expr::Term(Term::Path("primo semestre".into()))));
    assert_eq!(parse_query("file:MST"), Some(Expr::Term(Term::File("mst".into()))));
    assert_eq!(parse_query("tag:#Uni/Algo"), Some(Expr::Term(Term::Tag("uni/algo".into()))));
    assert_eq!(parse_query("tag:esame"), Some(Expr::Term(Term::Tag("esame".into()))));
    assert_eq!(parse_query("content:x"), Some(Expr::Term(Term::Content("x".into()))));
    assert_eq!(parse_query("-path:Altro"), Some(Expr::Not(Box::new(Expr::Term(Term::Path("altro".into()))))));
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
        doc("Uni/Algoritmi/The MST problem.md", "# MST\nIl problema del Minimum Spanning Tree.\nUsa Borůvka.\n", &["uni/algo"]),
        doc("Uni/Calcolabilità.md", "Macchine di Turing\nCalcolabilità e decidibilità\n", &["uni"]),
        doc("Altro/Thesis.md", "Planning the thesis.\nA tree of ideas, tree everywhere.\n", &[]),
        doc("b.md", "nothing here", &["uniform"]),
    ]
}

#[test]
fn matching_is_case_insensitive_and_and_by_default() {
    let v = vault();
    assert_eq!(paths(&run("tree", &v)), vec!["Uni/Algoritmi/The MST problem.md", "Altro/Thesis.md"]);
    assert_eq!(paths(&run("TREE minimum", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
    assert_eq!(paths(&run("calcolabilità", &v)), vec!["Uni/Calcolabilità.md"]);
    assert_eq!(paths(&run("borůvka", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
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
    // "thesis" is in Thesis.md's text and path; "altro" only in its path
    assert_eq!(paths(&run("altro", &v)), vec!["Altro/Thesis.md"]);
    let hits = run("altro", &v);
    assert!(hits[0].lines.is_empty(), "a path-only match lists no lines");
}

#[test]
fn phrases_or_not_and_operators() {
    let v = vault();
    assert_eq!(paths(&run("\"spanning tree\"", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
    assert!(run("\"tree spanning\"", &v).is_empty());
    assert_eq!(paths(&run("turing OR borůvka", &v)), vec!["Uni/Calcolabilità.md", "Uni/Algoritmi/The MST problem.md"]);
    assert_eq!(paths(&run("tree -thesis", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
    assert_eq!(paths(&run("tree path:altro", &v)), vec!["Altro/Thesis.md"]);
    assert_eq!(paths(&run("file:mst", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
    assert!(run("file:algoritmi", &v).is_empty(), "file: is the file name only");
    assert_eq!(paths(&run("content:altro", &v)), Vec::<&str>::new());
    assert_eq!(paths(&run("-path:uni", &v)), vec!["b.md", "Altro/Thesis.md"]);
}

#[test]
fn tags_match_exactly_or_as_parent() {
    let v = vault();
    assert_eq!(paths(&run("tag:#uni", &v)), vec!["Uni/Calcolabilità.md", "Uni/Algoritmi/The MST problem.md"]);
    assert_eq!(paths(&run("tag:uni/algo", &v)), vec!["Uni/Algoritmi/The MST problem.md"]);
    assert!(run("tag:algo", &v).is_empty(), "only whole leading segments match");
    assert_eq!(paths(&run("tag:UNIFORM", &v)), vec!["b.md"]);
}

#[test]
fn line_hits_with_merged_ranges() {
    let text = "# MST\nIl problema del Minimum Spanning Tree.\nUsa Borůvka.\ntree, TREE e treetop\n";
    let v = vec![doc("n.md", text, &[])];
    let hits = run("tree OR borůvka", &v);
    assert_eq!(hits.len(), 1);
    let lines: Vec<(usize, usize, Vec<&str>)> = hits[0].lines.iter().map(|l| (l.line, l.start, l.ranges.iter().map(|r| &text[r.clone()]).collect())).collect();
    assert_eq!(
        lines,
        vec![
            (1, 6, vec!["Tree"]),
            (2, 45, vec!["Borůvka"]),
            (3, 59, vec!["tree", "TREE", "tree"]),
        ]
    );
    // overlapping matches of different terms merge into one range
    let v = vec![doc("n.md", "spanning tree", &[])];
    let hits = run("\"spanning tree\" tree", &v);
    assert_eq!(hits[0].lines[0].ranges, vec![0..13]);
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
    let text = "uno\r\ndue tre\r\ntre";
    let v = vec![doc("n.md", text, &[])];
    let hits = run("tre", &v);
    let lines: Vec<(usize, usize)> = hits[0].lines.iter().map(|l| (l.line, l.start)).collect();
    assert_eq!(lines, vec![(1, 5), (2, 14)]);
    assert_eq!(&text[hits[0].lines[0].ranges[0].clone()], "tre");
}

#[test]
fn large_vault_is_fast() {
    // ~1.2 MB like the fixture; one query must stay far below the 100 ms budget
    let para = "Sia G=(V,E) un grafo connesso; il minimum spanning tree si trova con Kruskal o Borůvka. ";
    let docs: Vec<SearchDoc> = (0..240).map(|i| doc(&format!("d{i}/n{i}.md"), &para.repeat(57), &[])).collect();
    let refs: Vec<&SearchDoc> = docs.iter().collect();
    let q = parse_query("grafo kruskal -dijkstra").unwrap();
    let t = std::time::Instant::now();
    let hits = search(&q, &refs);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(hits.len(), 240);
    assert!(ms < if cfg!(debug_assertions) { 500.0 } else { 50.0 }, "search took {ms:.1} ms");
}
