// Builds the two JS bundles compiled into the app (crates/app/assets/, gitignored):
//   mathjax-svg.js    MathJax 3 TeX -> SVG, run in the embedded QuickJS   (entry: tools/corpus/mathjax-entry.js)
//   cm6-headless.js   headless CM6 state + Lezer Markdown (GFM) for the LaTeX snippets plugin
//                     (entry: tools/cm6/entry.js)
// Usage: npm ci && npm run build            [-- <out dir>, default crates/app/assets]
// Cross-platform on purpose: this is what a fresh clone on Windows runs too.
import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const out = path.resolve(process.argv[2] ?? path.join(root, "crates/app/assets"));
const common = {
  bundle: true, minify: true, format: "iife", platform: "neutral", target: "es2020",
  mainFields: ["module", "main"], nodePaths: [path.join(here, "node_modules")], logLevel: "warning",
};

await build({
  ...common,
  entryPoints: [path.join(root, "tools/corpus/mathjax-entry.js")],
  outfile: path.join(out, "mathjax-svg.js"),
  define: { PACKAGE_VERSION: '"3.2.1"' },
  banner: { js: 'var require=function(m){throw new Error("require("+m+") unavailable")};var __dirname="/";' },
});
await build({
  ...common,
  entryPoints: [path.join(root, "tools/cm6/entry.js")],
  outfile: path.join(out, "cm6-headless.js"),
});
console.log(`bundles written to ${out}`);
