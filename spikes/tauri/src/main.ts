// Perf spike frontend: CM6 live preview with KaTeX math widgets and lazy image embeds.
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { EditorState, StateField, RangeSetBuilder } from "@codemirror/state";
import type { Extension, Transaction } from "@codemirror/state";
import { EditorView, Decoration, WidgetType } from "@codemirror/view";
import type { DecorationSet } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { keymap } from "@codemirror/view";
import katex from "katex";
import { findMathRanges } from "../../../tools/corpus/mathRanges.ts";

const mathCache = new Map<string, string>();
function renderTex(tex: string, display: boolean): string {
  const key = (display ? "D" : "I") + tex;
  let html = mathCache.get(key);
  if (html === undefined) {
    try {
      html = katex.renderToString(tex, { displayMode: display, throwOnError: true, macros: { "\\textemdash": "\\text{—}" } });
    } catch (e) {
      html = `<span class="math-error" title="${String(e).replace(/"/g, "&quot;")}">${tex.replace(/</g, "&lt;")}</span>`;
    }
    mathCache.set(key, html);
  }
  return html;
}

class MathWidget extends WidgetType {
  constructor(readonly tex: string, readonly display: boolean) { super(); }
  eq(o: MathWidget) { return o.tex === this.tex && o.display === this.display; }
  toDOM() {
    const el = document.createElement(this.display ? "div" : "span");
    el.className = this.display ? "math-block" : "math-inline";
    el.innerHTML = renderTex(this.tex, this.display);
    return el;
  }
  ignoreEvent() { return false; }
}

class ImageWidget extends WidgetType {
  constructor(readonly src: string) { super(); }
  eq(o: ImageWidget) { return o.src === this.src; }
  toDOM() {
    const d = document.createElement("div");
    d.className = "embed";
    const img = document.createElement("img");
    img.loading = "lazy";
    img.decoding = "async";
    img.src = this.src;
    d.appendChild(img);
    return d;
  }
  get estimatedHeight() { return 300; }
}

let vaultRoot = "";
function buildDecos(state: EditorState): DecorationSet {
  const text = state.doc.toString();
  const sel = state.selection.main;
  const items: { from: number; to: number; deco: Decoration }[] = [];
  for (const r of findMathRanges(text)) {
    if (sel.from <= r.to && sel.to >= r.from) continue; // cursor inside: show source
    const d = r.display ? 2 : 1;
    let tex = text.slice(r.from + d, r.to - d);
    tex = tex.replace(/\n[ \t]*(?:>[ \t]?)+/g, "\n");
    const isBlock = r.display && state.doc.lineAt(r.from).from === r.from && state.doc.lineAt(r.to).to === r.to;
    items.push({ from: r.from, to: r.to, deco: Decoration.replace({ widget: new MathWidget(tex, r.display), block: isBlock }) });
  }
  for (const m of text.matchAll(/!\[\[([^\]|#]+\.(?:png|jpe?g|gif|svg|webp))(?:\|[^\]]*)?\]\]/gi)) {
    const from = m.index!, to = from + m[0].length;
    if (sel.from <= to && sel.to >= from) continue;
    const line = state.doc.lineAt(from);
    const block = line.from === from && line.to === to;
    items.push({ from, to, deco: Decoration.replace({ widget: new ImageWidget(convertFileSrc(`${vaultRoot}/Images/${m[1]}`)), block }) });
  }
  items.sort((a, b) => a.from - b.from);
  const b = new RangeSetBuilder<Decoration>();
  for (const it of items) b.add(it.from, it.to, it.deco);
  return b.finish();
}

const livePreview: Extension = StateField.define<DecorationSet>({
  create: buildDecos,
  update(v: DecorationSet, tr: Transaction) { return tr.docChanged || tr.selection ? buildDecos(tr.state) : v; },
  provide: (f) => EditorView.decorations.from(f),
});

const raf = () => new Promise<number>((r) => requestAnimationFrame(r));
const pct = (xs: number[], p: number) => { const s = [...xs].sort((a, b) => a - b); return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(2); };
const summary = (xs: number[]) => ({ n: xs.length, p50: pct(xs, 0.5), p95: pct(xs, 0.95), max: pct(xs, 1), mean: +(xs.reduce((a, b) => a + b, 0) / xs.length).toFixed(2) });

async function typingBench(view: EditorView, at: number, text: string) {
  view.dispatch({ selection: { anchor: at }, scrollIntoView: true });
  await raf(); await raf();
  const sync: number[] = [], frames: number[] = [];
  for (const ch of text) {
    const t0 = performance.now();
    const pos = view.state.selection.main.head;
    view.dispatch({ changes: { from: pos, insert: ch }, selection: { anchor: pos + 1 }, userEvent: "input.type" });
    void view.dom.getBoundingClientRect(); // force style+layout
    sync.push(performance.now() - t0);
    await raf(); // CM6 measure phase + paint
    frames.push(performance.now() - t0);
  }
  return { sync: summary(sync), toNextFrame: summary(frames) };
}

async function scrollBench(view: EditorView) {
  const sc = view.scrollDOM;
  sc.scrollTop = 0; await raf(); await raf();
  const steps = 150, max = sc.scrollHeight - sc.clientHeight;
  const iv: number[] = [];
  let last = await raf();
  for (let i = 1; i <= steps; i++) { sc.scrollTop = (max * i) / steps; const t = await raf(); iv.push(t - last); last = t; }
  return { ...summary(iv), slowFrames: iv.filter((x) => x > 25).length, scrollHeight: sc.scrollHeight };
}

async function main() {
  const t0 = performance.now();
  const cfg = await invoke<{ vault: string; note: string; bench: boolean }>("config");
  vaultRoot = cfg.vault;
  const doc = await invoke<string>("load_note", { rel: cfg.note });
  const view = new EditorView({
    parent: document.getElementById("app")!,
    state: EditorState.create({ doc, extensions: [markdown(), EditorView.lineWrapping, history(), keymap.of([...defaultKeymap, ...historyKeymap]), livePreview] }),
  });
  view.dispatch({ selection: { anchor: 0 } });
  await raf(); await raf();
  const firstRenderMs = performance.now() - t0;
  await invoke("ready");
  if (!cfg.bench) return;
  await new Promise((r) => setTimeout(r, 1000));
  // Type plain text in the middle of the note, then type inside a formula, then leave it (re-render).
  const mid = view.state.doc.lineAt(Math.floor(doc.length / 2)).to;
  const plain = await typingBench(view, mid, " the quick brown fox jumps over the lazy dog".repeat(3));
  const ranges = findMathRanges(view.state.doc.toString());
  const target = ranges[Math.floor(ranges.length / 2)];
  const inMath = await typingBench(view, target.to - (target.display ? 2 : 1), "+\\frac{a}{b}^{2}");
  // Leave the formula: forces a KaTeX render of a new (uncached) formula.
  const tLeave = performance.now();
  view.dispatch({ selection: { anchor: 0 } });
  void view.dom.getBoundingClientRect();
  const leaveMs = performance.now() - tLeave;
  const scroll = await scrollBench(view);
  await invoke("report", { json: JSON.stringify({ engine: "tauri-webkitgtk", note: cfg.note, docChars: doc.length, formulas: ranges.length, firstRenderMs: +firstRenderMs.toFixed(1), plain, inMath, leaveFormulaMs: +leaveMs.toFixed(2), scroll, cachedFormulas: mathCache.size }) });
}
main().catch((e) => invoke("report", { json: JSON.stringify({ error: String(e) }) }));
