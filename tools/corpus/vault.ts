// Corpus helpers: walk a vault and extract math, embeds, links, callouts, etc.
// Used by stats.ts (HANDOFF §5 figures) and math.ts (HANDOFF §7 corpus math test).
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

import { findMathRanges } from "./mathRanges.ts";
export { findMathRanges, type MathRange } from "./mathRanges.ts";
export interface Formula { file: string; line: number; display: boolean; tex: string; ctx: "plain" | "callout" | "quote" | "list" | "table" }
export interface Note { path: string; text: string }

export function walkVault(root: string): Note[] {
  const out: Note[] = [];
  const rec = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.name.startsWith(".")) continue; // .obsidian, .stfolder, app config
      const p = join(dir, e.name);
      if (e.isDirectory()) rec(p);
      else if (e.name.endsWith(".md")) out.push({ path: relative(root, p), text: readFileSync(p, "utf8") });
    }
  };
  rec(root);
  return out.sort((a, b) => a.path.localeCompare(b.path));
}

export function listFiles(root: string): string[] {
  const out: string[] = [];
  const rec = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.name.startsWith(".")) continue;
      const p = join(dir, e.name);
      if (e.isDirectory()) rec(p); else out.push(relative(root, p));
    }
  };
  rec(root);
  return out;
}

export function lineStarts(text: string): number[] {
  const s = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") s.push(i + 1);
  return s;
}
export function lineOf(starts: number[], pos: number): number {
  let lo = 0, hi = starts.length - 1;
  while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (starts[mid] <= pos) lo = mid; else hi = mid - 1; }
  return lo;
}

export function extractFormulas(note: Note): Formula[] {
  const starts = lineStarts(note.text);
  return findMathRanges(note.text).map((r) => {
    const line = lineOf(starts, r.from);
    const lineText = note.text.slice(starts[line], starts[line + 1] ?? note.text.length);
    const d = r.display ? 2 : 1;
    let tex = note.text.slice(r.from + d, r.to - d);
    // Strip blockquote/callout prefixes on continuation lines, as Obsidian does.
    if (/^\s{0,3}>/.test(lineText)) tex = tex.replace(/\n[ \t]*(?:>[ \t]?)+/g, "\n");
    let ctx: Formula["ctx"] = "plain";
    if (/^\s{0,3}>/.test(lineText)) ctx = /\[!/.test(note.text.slice(starts[Math.max(0, line - 40)], starts[line + 1] ?? note.text.length)) ? "callout" : "quote";
    else if (/^\s*\|/.test(lineText)) ctx = "table";
    else if (/^\s*([-*+]|\d+[.)])\s/.test(lineText)) ctx = "list";
    // Obsidian table cells: "\|" is an escaped pipe inside math.
    if (ctx === "table") tex = tex.replace(/\\\|/g, "|");
    return { file: note.path, line: line + 1, display: r.display, tex, ctx };
  });
}

export function stripCode(text: string): string {
  // Blank out fenced code blocks and inline code (keeps offsets).
  const blank = (s: string) => s.replace(/[^\n]/g, " ");
  return text
    .replace(/^(\s{0,3}(?:>\s?)*)(`{3,}|~{3,})[^\n]*\n[\s\S]*?(?:^\s{0,3}(?:>\s?)*\2[^\n]*$|(?![\s\S]))/gm, blank)
    .replace(/(`+)[^`\n]*?\1/g, blank);
}

export function stripMath(text: string): string {
  let out = text;
  for (const r of findMathRanges(text).reverse()) out = out.slice(0, r.from) + " ".repeat(r.to - r.from) + out.slice(r.to);
  return out;
}
