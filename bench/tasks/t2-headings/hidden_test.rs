use bench::*;
fn h(level: u8, text: &str, line: usize, children: Vec<Heading>) -> Heading { Heading { level, text: text.into(), line, children } }
#[test] fn no_frontmatter() { let p = parse("# A\ntext\n"); assert_eq!(p.frontmatter, None); assert_eq!(p.body_offset, 0); assert_eq!(p.headings, vec![h(1,"A",0,vec![])]); }
#[test] fn frontmatter() { let s = "---\ncssclasses: NoBorder\ntags: [a]\n---\n# Title\n"; let p = parse(s); assert_eq!(p.frontmatter.as_deref(), Some("cssclasses: NoBorder\ntags: [a]")); assert_eq!(p.body_offset, s.find("# Title").unwrap()); assert_eq!(p.headings, vec![h(1,"Title",4,vec![])]); }
#[test] fn empty_frontmatter() { let p = parse("---\n---\nx"); assert_eq!(p.frontmatter.as_deref(), Some("")); assert_eq!(p.body_offset, 8); }
#[test] fn unclosed_frontmatter() { let p = parse("---\n# A\n"); assert_eq!(p.frontmatter, None); assert_eq!(p.headings.len(), 1); }
#[test] fn tree() { let p = parse("# A\n## B\n### C\n## D\n# E\n"); assert_eq!(p.headings, vec![h(1,"A",0,vec![h(2,"B",1,vec![h(3,"C",2,vec![])]),h(2,"D",3,vec![])]),h(1,"E",4,vec![])]); }
#[test] fn skip_levels() { let p = parse("# A\n### B\n## C\n"); assert_eq!(p.headings, vec![h(1,"A",0,vec![h(3,"B",1,vec![]),h(2,"C",2,vec![])])]); }
#[test] fn starts_deep() { let p = parse("### A\n# B\n"); assert_eq!(p.headings, vec![h(3,"A",0,vec![]),h(1,"B",1,vec![])]); }
#[test] fn tag_not_heading() { assert!(parse("#tag\n#esame\n").headings.is_empty()); }
#[test] fn seven_hashes() { assert!(parse("####### x\n").headings.is_empty()); }
#[test] fn closing_seq() { let p = parse("## Foo ##\n## C#\n##   Bar   \n"); let t: Vec<_> = p.headings.iter().map(|x| x.text.clone()).collect(); assert_eq!(t, vec!["Foo","C#","Bar"]); }
#[test] fn code_fence() { let p = parse("# A\n```c\n# include\n```\n~~~~\n# no\n~~~\n# still no\n~~~~\n# B\n"); let t: Vec<_> = p.headings.iter().map(|x| (x.text.clone(), x.line)).collect(); assert_eq!(t, vec![("A".to_string(),0),("B".to_string(),9)]); }
#[test] fn unclosed_fence() { assert_eq!(parse("# A\n```\n# B\n").headings.len(), 1); }
#[test] fn crlf() { let p = parse("---\r\na: 1\r\n---\r\n# Ciao à tutti\r\n"); assert_eq!(p.frontmatter.as_deref(), Some("a: 1")); assert_eq!(p.headings[0].text, "Ciao à tutti"); assert_eq!(p.headings[0].line, 3); }
#[test] fn indented() { assert_eq!(parse("   # A\n    # B\n").headings.len(), 1); }
#[test] fn empty_heading() { let p = parse("#\n# \n"); assert_eq!(p.headings.len(), 2); assert_eq!(p.headings[0].text, ""); }
#[test] fn unicode_lines() { let p = parse("Testo con àèìòù\n## Borůvka\n"); assert_eq!(p.headings, vec![h(2,"Borůvka",1,vec![])]); }
