Write the TypeScript file `src/highlight.ts`: a CodeMirror 6 extension that decorates Obsidian `==highlight==` syntax.
Allowed imports: `@codemirror/state` and `@codemirror/view` only. TypeScript strict mode, ES modules.

Required exports (exact names and types):

```ts
import type { EditorState } from "@codemirror/state";
import { ViewPlugin, DecorationSet } from "@codemirror/view";

export interface HighlightRange { from: number; to: number }   // offsets INCLUDING the == markers

/** Pure scanner over the whole text. */
export function findHighlights(text: string): HighlightRange[];

/** ViewPlugin whose instance exposes `decorations: DecorationSet`. */
export const highlightPlugin: ViewPlugin<{ decorations: DecorationSet }>;
```

Scanner rules:
- A highlight starts at `==` and ends at the next `==` on the SAME line. The inner text must be non-empty. If no closing `==` on the line, the opening `==` is literal and scanning continues after it.
- Highlights never span a newline.
- Text inside inline code spans (`` `...` `` — a run of N backticks closed by a run of exactly N backticks on the same line) is ignored, as are lines inside fenced code blocks (lines starting with ``` toggle a fence).
- `===` sequences: scan left to right; the first `==` found opens. Ranges are returned in document order and never overlap.

Plugin rules:
- Build decorations with `Decoration.mark` for every range from `findHighlights(view.state.doc.toString())`:
  - class `cm-highlight-marker` on each of the two `==` markers,
  - class `cm-highlight` on the inner text.
- Recompute when the document changes (`update.docChanged`). Decorations must be sorted (use `RangeSetBuilder` or `Decoration.set(..., true)`).
- Pass `{ decorations: v => v.decorations }` as the plugin spec.
