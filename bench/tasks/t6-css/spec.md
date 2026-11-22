Write the stylesheet `styles/callouts.css` for Obsidian-style callouts. Plain CSS, no preprocessors, no @import.

Rendered markup:
```html
<div class="callout" data-callout="thm" data-callout-fold="-">   <!-- class "is-collapsed" added when folded -->
  <div class="callout-title">
    <div class="callout-icon"></div>
    <div class="callout-title-inner">Teorema 1.2</div>
    <div class="callout-fold"></div>
  </div>
  <div class="callout-content">...</div>
</div>
```

Requirements:
1. Each callout type sets the custom property `--callout-color` to an RGB triplet written as three comma-separated integers (e.g. `--callout-color: 68, 138, 255;`) in a rule whose selector contains `.callout[data-callout="TYPE"]`. Required types: note, info, tip, warning, danger, quote, def, thm, lem, cor, prop, rem, pf, proof, algorithm. `pf` and `proof` may share one rule (comma-separated selectors).
2. The base `.callout` rule sets a default `--callout-color`, `background-color: rgba(var(--callout-color), 0.1)`, and `border-left: 4px solid rgb(var(--callout-color))`. It uses `var(--radius-m)` for border-radius and `var(--size-4-3)` (with a fallback) for padding.
3. `.callout-title` uses `color: rgb(var(--callout-color))`, `font-weight: var(--font-semibold)`, display flex, align-items center, gap.
4. `.callout-content` uses `color: var(--text-normal)`.
5. `.callout.is-collapsed .callout-content { display: none; }`. `.callout-fold` gets `cursor: pointer` and rotates (`transform: rotate(-90deg)`) when the callout is collapsed.
6. `pf`/`proof` callouts: no background (`background-color: transparent`) and the title is italic.
7. Colors: apart from the `--callout-color` triplets, NEVER write literal colors (no hex, no named colors, no rgb() with numbers) — use CSS variables only.
