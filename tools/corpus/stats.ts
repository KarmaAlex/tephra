// Vault statistics for HANDOFF §5. Prints aggregate JSON only (no note content).
// usage: node --import tsx stats.ts ~/dev/vault-fixture
import { walkVault, listFiles, extractFormulas, stripCode, stripMath } from "./vault.ts";

const root = process.argv[2];
if (!root) { console.error("usage: stats.ts <vault>"); process.exit(2); }
const notes = walkVault(root);
const files = listFiles(root);
const inc = (m: Record<string, number>, k: string, n = 1) => { m[k] = (m[k] ?? 0) + n; };

const s = {
  notes: notes.length,
  files_by_ext: {} as Record<string, number>,
  bytes_total_md: 0,
  largest_notes_bytes: [] as number[],
  frontmatter_notes: 0,
  cssclasses: {} as Record<string, number>,
  math_inline: 0, math_display: 0,
  math_by_ctx: {} as Record<string, number>,
  math_envs: {} as Record<string, number>,
  math_macros_of_interest: {} as Record<string, number>,
  embeds: 0, embeds_by_ext: {} as Record<string, number>, embeds_with_size: 0,
  wikilinks: 0, heading_links: 0, block_links: 0, aliased_links: 0,
  md_links_internal: 0, md_links_external: 0,
  block_ids: 0,
  table_rows: 0,
  headings: 0,
  callouts: {} as Record<string, number>, callouts_folded: 0, callouts_nested: 0,
  code_langs: {} as Record<string, number>,
  tags: {} as Record<string, number>,
  highlights: 0,
  comments_obsidian: 0,
  footnotes: 0,
  sync_conflicts: files.filter((f) => f.includes(".sync-conflict-")).length,
};
for (const f of files) inc(s.files_by_ext, (f.match(/\.([^./]+)$/)?.[1] ?? "(none)").toLowerCase());
const sizes: number[] = [];
for (const n of notes) {
  const t = n.text;
  sizes.push(Buffer.byteLength(t));
  s.bytes_total_md += Buffer.byteLength(t);
  const fm = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(t);
  if (fm) {
    s.frontmatter_notes++;
    const cc = /cssclass(?:es)?:\s*(.*(?:\n\s+-.*)*)/.exec(fm[1]);
    if (cc) for (const c of cc[1].replace(/[\[\]\-,]/g, " ").split(/\s+/).filter(Boolean)) inc(s.cssclasses, c);
  }
  for (const f of extractFormulas(n)) {
    f.display ? s.math_display++ : s.math_inline++;
    inc(s.math_by_ctx, `${f.ctx}/${f.display ? "display" : "inline"}`);
    for (const m of f.tex.matchAll(/\\begin\{([^}]+)\}/g)) inc(s.math_envs, m[1]);
    for (const m of f.tex.matchAll(/\\(mathbb|mathcal|mathfrak|mathscr|operatorname|text|textbf|newcommand|def|require|color|boxed|cancel|xrightarrow|overset|underset|tag|label|ref|eqref)\b/g)) inc(s.math_macros_of_interest, m[1]);
  }
  const code = stripCode(t);
  for (const m of t.matchAll(/^\s{0,3}(?:>\s?)*(?:`{3,}|~{3,})\s*([\w+#-]*)/gm)) inc(s.code_langs, m[1] || "(none)");
  // code fence lines are counted twice (open+close); fix below by halving "(none)" heuristically is wrong, so count opens only:
  const plain = stripMath(code);
  for (const m of plain.matchAll(/!\[\[([^\]]+)\]\]/g)) {
    s.embeds++;
    const target = m[1].split("|")[0].split("#")[0];
    inc(s.embeds_by_ext, (target.match(/\.([^./]+)$/)?.[1] ?? "md").toLowerCase());
    if (/\|\s*\d+/.test(m[1])) s.embeds_with_size++;
  }
  for (const m of plain.matchAll(/(?<!!)\[\[([^\]]+)\]\]/g)) {
    s.wikilinks++;
    if (m[1].includes("#^")) s.block_links++; else if (m[1].includes("#")) s.heading_links++;
    if (m[1].includes("|")) s.aliased_links++;
  }
  for (const m of plain.matchAll(/(?<!!)\[[^\]\n]*\]\(([^)\s]+)[^)]*\)/g)) /^[a-z]+:/i.test(m[1]) ? s.md_links_external++ : s.md_links_internal++;
  s.block_ids += [...plain.matchAll(/\s\^[A-Za-z0-9-]+\s*$/gm)].length;
  s.table_rows += [...plain.matchAll(/^\s*(?:>\s?)*\|.*\|\s*$/gm)].length;
  s.headings += [...plain.matchAll(/^#{1,6}(\s|$)/gm)].length;
  for (const m of plain.matchAll(/^\s{0,3}((?:>\s*)+)\[!([^\]]+)\]([+-]?)/gm)) {
    inc(s.callouts, m[2].toLowerCase());
    if (m[3]) s.callouts_folded++;
    if ((m[1].match(/>/g) ?? []).length > 1) s.callouts_nested++;
  }
  for (const m of plain.replace(/\[\[[^\]]*\]\]|\]\([^)]*\)|https?:\/\/\S+/g, "").matchAll(/(?:^|\s)#([\p{L}\p{N}_/-]*[\p{L}_/-][\p{L}\p{N}_/-]*)/gmu)) inc(s.tags, m[1]);
  s.highlights += [...plain.matchAll(/==[^=\n]+==/g)].length;
  s.comments_obsidian += [...plain.matchAll(/%%[\s\S]*?%%/g)].length;
  s.footnotes += [...plain.matchAll(/\[\^[^\]]+\]/g)].length;
}
// Fence opens: every fence line matched above, so halve per language conservatively:
const langs: Record<string, number> = {};
for (const n of notes) {
  let open: string | null = null;
  for (const line of n.text.split("\n")) {
    const m = /^\s{0,3}(?:>\s?)*(`{3,}|~{3,})\s*([\w+#-]*)/.exec(line);
    if (!m) continue;
    if (open === null) { open = m[1]; inc(langs, m[2] || "(none)"); }
    else if (m[1][0] === open[0] && m[1].length >= open.length && !m[2]) open = null;
  }
}
s.code_langs = langs;
s.largest_notes_bytes = sizes.sort((a, b) => b - a).slice(0, 5);
const tagCount = Object.keys(s.tags).length;
console.log(JSON.stringify({ ...s, tags_distinct: tagCount, tags: undefined, tag_uses: Object.values(s.tags).reduce((a, b) => a + b, 0) }, null, 2));
