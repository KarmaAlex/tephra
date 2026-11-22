// HANDOFF §7 corpus math test: render every formula in the vault with KaTeX and MathJax 3.
// usage: node --import tsx math.ts <vault> [--engine katex|mathjax|both] [--hybrid]
// Aggregates go to stdout; per-failure details (file, line, error, tex) go to
// reports/ (gitignored — contains note content).
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import katex from "katex";
import { mathjax } from "mathjax-full/js/mathjax.js";
import { TeX } from "mathjax-full/js/input/tex.js";
import { SVG } from "mathjax-full/js/output/svg.js";
import { liteAdaptor } from "mathjax-full/js/adaptors/liteAdaptor.js";
import { RegisterHTMLHandler } from "mathjax-full/js/handlers/html.js";
import { AllPackages } from "mathjax-full/js/input/tex/AllPackages.js";
import { walkVault, extractFormulas, type Formula } from "./vault.ts";

const args = process.argv.slice(2);
const root = args[0];
const engine = args.includes("--engine") ? args[args.indexOf("--engine") + 1] : "both";
const reportDir = join(import.meta.dirname, "../../reports");
mkdirSync(reportDir, { recursive: true });

export const KATEX_MACROS: Record<string, string> = {};

export function renderKatex(f: Formula, warnings: string[] = []): string {
  return katex.renderToString(f.tex, {
    displayMode: f.display, throwOnError: true, output: "html", macros: { ...KATEX_MACROS },
    strict: (code: string) => { warnings.push(code); return "ignore"; },
  });
}

const adaptor = liteAdaptor();
RegisterHTMLHandler(adaptor);
const tex = new TeX({
  packages: AllPackages.filter((p) => !["bussproofs", "noundefined", "noerrors", "require", "autoload"].includes(p)),
  formatError: (_jax: unknown, err: Error) => { throw err; },
});
const doc = mathjax.document("", { InputJax: tex, OutputJax: new SVG({ fontCache: "local" }) });
export function renderMathjax(f: Formula): string {
  const node = doc.convert(f.tex, { display: f.display });
  const html = adaptor.outerHTML(node);
  if (html.includes("data-mjx-error") || html.includes("merror")) throw new Error("merror in output");
  return html;
}

interface Result { engine: string; total: number; failures: number; byError: Record<string, number>; byCtx: Record<string, number>; ms: number; warnings: Record<string, number> }

function run(name: string, formulas: Formula[], fn: (f: Formula, w: string[]) => string): Result {
  const res: Result = { engine: name, total: formulas.length, failures: 0, byError: {}, byCtx: {}, ms: 0, warnings: {} };
  const details: object[] = [];
  const t0 = performance.now();
  for (const f of formulas) {
    const w: string[] = [];
    try { fn(f, w); } catch (e) {
      res.failures++;
      const msg = String((e as Error).message ?? e).split("\n")[0];
      const cat = msg.replace(/'[^']*'|"[^"]*"|\\[A-Za-z]+|\{[^}]*\}|at position \d+:.*$|\d+/g, "…").slice(0, 90);
      res.byError[cat] = (res.byError[cat] ?? 0) + 1;
      res.byCtx[`${f.ctx}/${f.display ? "display" : "inline"}`] = (res.byCtx[`${f.ctx}/${f.display ? "display" : "inline"}`] ?? 0) + 1;
      details.push({ file: f.file, line: f.line, display: f.display, ctx: f.ctx, error: msg, tex: f.tex });
    }
    for (const c of w) res.warnings[c] = (res.warnings[c] ?? 0) + 1;
  }
  res.ms = Math.round(performance.now() - t0);
  writeFileSync(join(reportDir, `math-corpus-${name}.jsonl`), details.map((d) => JSON.stringify(d)).join("\n") + "\n");
  return res;
}

const notes = walkVault(root);
const formulas = notes.flatMap(extractFormulas);
const out: Result[] = [];
if (engine === "katex" || engine === "both") out.push(run("katex", formulas, renderKatex));
if (engine === "mathjax" || engine === "both") out.push(run("mathjax", formulas, (f) => renderMathjax(f)));
if (args.includes("--hybrid")) out.push(run("hybrid", formulas, (f, w) => { try { return renderKatex(f, w); } catch { return renderMathjax(f); } }));
// second KaTeX pass = warm timing (JIT warmed up)
if (engine === "both") { const r = run("katex-warm", formulas, renderKatex); out.push(r); }
console.log(JSON.stringify({ notes: notes.length, formulas: formulas.length, results: out }, null, 2));
