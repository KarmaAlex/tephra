Write the TypeScript file `src/mathRanges.ts` (strict mode, ES module, no imports) that finds LaTeX math ranges in Obsidian Markdown.

```ts
export interface MathRange { from: number; to: number; display: boolean } // offsets INCLUDING delimiters
export function findMathRanges(md: string): MathRange[];
```

Rules (scan left to right; results in document order, never overlapping):
1. Fenced code blocks are skipped entirely: a line whose first non-space characters (at most 3 leading spaces) are ``` or ~~~ opens a fence; it closes at the next line that starts (after at most 3 spaces) with the same fence character repeated at least as many times. Unclosed fences run to end of input.
2. Inline code spans are skipped: a run of N backticks opens a span closed by the next run of exactly N backticks (anywhere later, may cross lines). If there is no closing run, the backticks are literal text.
3. A `$` preceded by an odd number of consecutive backslashes is escaped and is never a delimiter.
4. `$$` opens DISPLAY math, closed by the next unescaped `$$` (may span lines; nothing inside display math is special except escaped `\$`). If there is no closing `$$`, the opening `$$` is literal text and scanning continues after it.
5. A single `$` opens INLINE math only if the next character exists and is not whitespace. It closes at the next unescaped `$` on the same line such that the character before it is not whitespace and the character after it is not a digit (0-9). If no valid closing `$` exists on that line, the opening `$` is literal and scanning continues at the next character.
6. Inside math (inline or display), backticks are not special.
