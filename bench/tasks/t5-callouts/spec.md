Write the TypeScript file `src/callouts.ts` (strict mode, ES module, no imports) that parses Obsidian callouts.

```ts
export interface Callout {
  type: string;              // lowercased, e.g. "thm"
  title: string;             // trimmed text after the marker, "" if none
  fold: "+" | "-" | null;    // fold marker directly after "]"
  startLine: number;         // 0-based line of the header
  endLine: number;           // 0-based, inclusive: last line belonging to the callout
  depth: number;             // number of ">" markers on the header line
  children: Callout[];       // callouts nested inside this one
}
export function parseCallouts(md: string): Callout[];   // top-level callouts in document order
```

Rules:
- Lines split on "\n" (ignore a trailing "\r").
- Quote depth of a line: after at most 3 leading spaces, count consecutive `>` markers, where each `>` may be followed by optional spaces before the next `>`. A line with no leading `>` has depth 0. The line's content is what remains after the last `>` and ONE optional following space.
- A callout header is a line with depth d >= 1 whose content matches `[!TYPE]` immediately followed by an optional `+` or `-`, then optional title text. TYPE is one or more characters other than `]` (letters, digits, `-`, `_`). Example: `> [!thm]- Teorema di Bolzano` -> type "thm", fold "-", title "Teorema di Bolzano". `[!def]Title` (no space) gives title "Title".
- A callout started at depth d continues on following lines while their depth is >= d. It ends before the first line with depth < d (blank lines without `>` have depth 0 and therefore end it).
- A line is a header only if it is the FIRST line of a quote at its depth: it is line 0, or the previous line's depth is less than d. (So `> text` followed by `> [!thm] x` is a plain quote, not a callout, and a second `> [!x]` line inside the same quote is ordinary content.)
- A header at depth d' inside an open callout of depth d < d' is a child of the innermost open callout with depth < d'.
- Lines inside fenced code blocks that are NOT quoted (depth 0 lines starting with ``` or ~~~ toggle a fence) are never callout headers.
