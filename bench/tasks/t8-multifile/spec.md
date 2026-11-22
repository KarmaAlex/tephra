Create a tiny TypeScript package with EXACTLY these four files (strict mode, ES modules, no dependencies):

1. `src/slug.ts` exporting `export function slugify(s: string): string`:
   - Unicode-normalize with NFD and remove combining diacritical marks (U+0300–U+036F), so "Calcolabilità" -> "calcolabilita".
   - Lowercase.
   - Replace every run of characters that are not `a-z` or `0-9` with a single `-`.
   - Remove leading and trailing `-`.
2. `src/wordcount.ts` exporting `export function wordCount(s: string): number`: the number of maximal runs of non-whitespace characters (`/\S+/g`). Empty or all-whitespace string -> 0.
3. `src/index.ts` that re-exports both functions: `export { slugify } from "./slug.ts";` and `export { wordCount } from "./wordcount.ts";`
4. `README.md` starting with the line `# text-utils`, followed by a blank line and a bullet list with one bullet per exported function (the bullet starts with the function name in backticks).
