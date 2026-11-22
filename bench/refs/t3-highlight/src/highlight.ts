import type { EditorState } from "@codemirror/state";
import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, ViewPlugin, EditorView } from "@codemirror/view";
import type { DecorationSet, ViewUpdate } from "@codemirror/view";
export interface HighlightRange { from: number; to: number }
export function findHighlights(text: string): HighlightRange[] {
  const out: HighlightRange[] = [];
  let fence = false, off = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("```")) { fence = !fence; off += line.length + 1; continue; }
    if (!fence) {
      let i = 0;
      while (i < line.length) {
        if (line[i] === "`") {
          let n = 0; while (line[i + n] === "`") n++;
          let j = i + n, close = -1;
          while (j < line.length) { if (line[j] === "`") { let m = 0; while (line[j + m] === "`") m++; if (m === n) { close = j + m; break; } j += m; } else j++; }
          i = close >= 0 ? close : i + n; continue;
        }
        if (line.startsWith("==", i)) {
          const c = line.indexOf("==", i + 2);
          if (c > i + 2) { out.push({ from: off + i, to: off + c + 2 }); i = c + 2; continue; }
          i += 2; continue;
        }
        i++;
      }
    }
    off += line.length + 1;
  }
  return out;
}
const marker = Decoration.mark({ class: "cm-highlight-marker" });
const inner = Decoration.mark({ class: "cm-highlight" });
function build(state: EditorState): DecorationSet {
  const b = new RangeSetBuilder<Decoration>();
  for (const r of findHighlights(state.doc.toString())) { b.add(r.from, r.from + 2, marker); b.add(r.from + 2, r.to - 2, inner); b.add(r.to - 2, r.to, marker); }
  return b.finish();
}
export const highlightPlugin = ViewPlugin.fromClass(class { decorations: DecorationSet; constructor(v: EditorView) { this.decorations = build(v.state); } update(u: ViewUpdate) { if (u.docChanged) this.decorations = build(u.state); } }, { decorations: (v) => v.decorations });
