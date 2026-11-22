// PROTOTYPE (throwaway): Obsidian note -> Typst markup + one SVG per formula (MathJax).
// The real pipeline would be Rust (same parser as the editor) with RaTeX- or MathJax-SVG math.
// usage (from tools/corpus, which has mathjax-full):
//   node --import tsx ../../spikes/pdf/convert.ts <vault> <note.md> <outdir>
// <outdir> receives main.typ, m*.svg and an Images symlink. It contains note content: keep it
// out of the repo.
import { mkdirSync, readFileSync, writeFileSync, symlinkSync, existsSync } from "node:fs";
import { basename, join } from "node:path";
import { mathjax } from "mathjax-full/js/mathjax.js";
import { TeX } from "mathjax-full/js/input/tex.js";
import { SVG } from "mathjax-full/js/output/svg.js";
import { liteAdaptor } from "mathjax-full/js/adaptors/liteAdaptor.js";
import { RegisterHTMLHandler } from "mathjax-full/js/handlers/html.js";
import { AllPackages } from "mathjax-full/js/input/tex/AllPackages.js";
import { findMathRanges } from "../../tools/corpus/mathRanges.ts";

const [vault, notePath, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
if (!existsSync(join(out, "Images"))) symlinkSync(join(vault, "Images"), join(out, "Images"));

const adaptor = liteAdaptor();
RegisterHTMLHandler(adaptor);
const mj = mathjax.document("", {
  InputJax: new TeX({ packages: AllPackages.filter((p) => !["bussproofs", "require", "autoload"].includes(p)) }),
  OutputJax: new SVG({ fontCache: "none" }),
});
const EX_EM = 0.442; // MathJax ex in em (Computer Modern x-height)
let mathN = 0;
function math(tex: string, display: boolean): string {
  const html = adaptor.outerHTML(mj.convert(tex, { display }));
  const svg = html.match(/<svg[\s\S]*<\/svg>/)![0].replace(/currentColor/g, "#000");
  const h = parseFloat(/height="([\d.]+)ex"/.exec(svg)![1]);
  const va = parseFloat(/vertical-align:\s*(-?[\d.]+)ex/.exec(svg)?.[1] ?? "0");
  const f = `m${mathN++}.svg`;
  writeFileSync(join(out, f), svg);
  const img = `image("${f}", height: ${(h * EX_EM).toFixed(3)}em)`;
  return display ? `#align(center, block(${img}))` : `#box(baseline: ${(-va * EX_EM).toFixed(3)}em, ${img})`;
}

const str = (s: string) => (s ? `#"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"` : "");

// Inline: math first (by offsets), then a small tokenizer for the rest.
function inline(text: string): string {
  let res = "", last = 0;
  for (const r of findMathRanges(text)) {
    res += inlineMd(text.slice(last, r.from));
    const d = r.display ? 2 : 1;
    res += math(text.slice(r.from + d, r.to - d), r.display);
    last = r.to;
  }
  return res + inlineMd(text.slice(last));
}
function inlineMd(t: string): string {
  const re = /!\[\[([^\]|#]+)(?:\|[^\]]*)?\]\]|\[\[([^\]]+)\]\]|\*\*(.+?)\*\*|__(.+?)__|==(.+?)==|`([^`]+)`|\*(.+?)\*|(?<![\w])_(.+?)_(?![\w])|\[([^\]]+)\]\(([^)]+)\)/g;
  let res = "", last = 0, m: RegExpExecArray | null;
  while ((m = re.exec(t))) {
    res += str(t.slice(last, m.index));
    if (m[1]) res += `#box(image("Images/${m[1]}", width: 60%))`;
    else if (m[2]) res += str(m[2].split("|").pop()!.split("#")[0]);
    else if (m[3] ?? m[4]) res += `#strong[${inlineMd(m[3] ?? m[4])}]`;
    else if (m[5]) res += `#highlight[${inlineMd(m[5])}]`;
    else if (m[6]) res += `#raw("${m[6].replace(/\\/g, "\\\\").replace(/"/g, '\\"')}")`;
    else if (m[7] ?? m[8]) res += `#emph[${inlineMd(m[7] ?? m[8])}]`;
    else if (m[9]) res += `#link("${m[10].replace(/"/g, "")}")[${inlineMd(m[9])}]`;
    last = re.lastIndex;
  }
  return res + str(t.slice(last));
}

// Blocks: a small line-based parser good enough for the test note.
function blocks(lines: string[]): string {
  const outp: string[] = [];
  let i = 0;
  const para: string[] = [];
  const flush = () => { if (para.length) { outp.push(para.map(inline).join(" #linebreak() ") + "\n"); para.length = 0; } };
  while (i < lines.length) {
    const l = lines[i];
    let m: RegExpExecArray | null;
    if ((m = /^(\s*)(```+|~~~+)\s*([\w+#-]*)/.exec(l))) {
      flush();
      const fence = m[2], lang = m[3].toLowerCase().replace("c++", "cpp");
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(fence)) body.push(lines[i++]);
      i++;
      outp.push(`#block(fill: luma(245), inset: 6pt, radius: 3pt, width: 100%, raw(block: true, lang: "${lang}", "${body.join("\n").replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"))\n`);
      continue;
    }
    if (l.trim().startsWith("$$")) {
      // display math possibly spanning lines
      flush();
      let buf = l.trim(), j = i;
      while (!(buf.length > 2 && /\$\$\s*$/.test(buf) && (buf.match(/\$\$/g) ?? []).length >= 2) && j + 1 < lines.length) buf += "\n" + lines[++j];
      outp.push(inline(buf) + "\n");
      i = j + 1;
      continue;
    }
    if ((m = /^(#{1,6})\s+(.*)$/.exec(l))) { flush(); outp.push(`#heading(level: ${m[1].length})[${inline(m[2])}]\n`); i++; continue; }
    if (/^\s*>/.test(l)) {
      flush();
      const q: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) q.push(lines[i++].replace(/^\s*>\s?/, ""));
      const c = /^\[!([^\]|]+)[^\]]*\]([+-]?)\s*(.*)$/.exec(q[0] ?? "");
      if (c) {
        const title = c[3] ? inline(c[3]) : str(c[1][0].toUpperCase() + c[1].slice(1));
        outp.push(`#block(stroke: (left: 2pt + rgb("#7c4dff")), fill: rgb("#7c4dff").transparentize(92%), inset: 8pt, radius: 3pt, width: 100%)[#strong[${title}]\n\n${blocks(q.slice(1))}]\n`);
      } else outp.push(`#quote(block: true)[${blocks(q)}]\n`);
      continue;
    }
    if (/^\s*\|.*\|\s*$/.test(l)) {
      flush();
      const rows: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) {
        const cells = lines[i++].trim().slice(1, -1).split(/(?<!\\)\|/).map((c) => c.trim());
        if (!cells.every((c) => /^:?-+:?$/.test(c))) rows.push(cells);
      }
      const n = Math.max(...rows.map((r) => r.length));
      outp.push(`#table(columns: ${n}, ${rows.map((r, ri) => r.concat(Array(n - r.length).fill("")).map((c) => ri === 0 ? `[#strong[${inline(c)}]]` : `[${inline(c)}]`).join(", ")).join(", ")})\n`);
      continue;
    }
    if ((m = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(l))) {
      flush();
      const depth = Math.floor(m[1].replace(/\t/g, "  ").length / 2);
      const marker = /\d/.test(m[2]) ? "+" : "-";
      outp.push(`${"  ".repeat(depth)}${marker} ${inline(m[3])}`);
      i++;
      if (!(i < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[i]))) outp.push("");
      continue;
    }
    if (!l.trim()) { flush(); i++; continue; }
    para.push(l.trim());
    i++;
  }
  flush();
  return outp.join("\n");
}

let src = readFileSync(join(vault, notePath), "utf8").replace(/\r\n/g, "\n");
src = src.replace(/^---\n[\s\S]*?\n---\n/, "");
const title = basename(notePath, ".md");
const t0 = performance.now();
const body = blocks(src.split("\n"));
const head = `#set document(title: ${str(title).slice(1)})
#set page(paper: "a4", margin: 10mm,
  header: context { set text(8pt, fill: luma(90)); ${str(title).slice(1)}; line(length: 100%, stroke: 0.4pt + luma(180)) },
  footer: context { set text(8pt, fill: luma(90)); align(center, counter(page).display("1 / 1", both: true)) })
#set text(size: 11pt, lang: "it")
#set par(justify: false, leading: 0.65em)
#show heading: set block(above: 1.1em, below: 0.6em)
`;
writeFileSync(join(out, "main.typ"), head + "\n" + body);
console.log(`CONVERT formulas=${mathN} ms=${(performance.now() - t0).toFixed(0)} typ_bytes=${(head + body).length}`);
