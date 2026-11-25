# AGENTS.md — Tephra

Contributor guide for this repository. Keep it current when the architecture or the
performance approach changes.

## Mission

Tephra is a lightweight, Obsidian-compatible desktop notes app. It is native
(Rust + GPUI) — no Electron and no bundled Chromium/CEF — and it opens existing
Obsidian vaults in place: `.obsidian/` is read-only, the app keeps its own settings in
`.tephra/` inside the vault. It provides a live-preview Markdown editor (like Obsidian's),
properly typeset math, and the features Obsidian gets from community plugins — search,
graph, backlinks, outline, tags, daily notes, templates, spellcheck, PDF export, … — built
in as toggleable first-party plugins. The goal is Obsidian-level functionality at a
fraction of Obsidian's memory and start-up time.

## Hard constraints (invariants)

- **No Electron / bundled Chromium.** The stack is Rust + GPUI (via `gpui-kit`, pinned in
  `Cargo.toml`). Deviations need a measured reason.
- **Vault safety.** `.obsidian/` is never written. Opening a note and saving it without
  edits must be byte-identical (CRLF/BOM/trailing whitespace preserved). Saves are atomic
  (temp + rename) and guarded by "disk bytes == the bytes we loaded/last saved"; on
  mismatch the app never clobbers a newer file — it reloads or reports a conflict.
- **External changes.** A vault-wide file watcher picks up changes made by other apps
  (sync tools, other editors); `*.sync-conflict-*` files are shown clearly.
- **Core stays platform-agnostic.** `crates/core` has no GUI dependencies so other
  front-ends remain possible.
- **Never commit vault data.** Fixtures and reports are gitignored; tests that use a local
  fixture (`$VAULT` or `~/dev/vault-fixture`) skip gracefully when it is absent.

## Architecture

```
crates/core    tephra-core — platform-agnostic core
               buffer.rs    Document: rope holding the file's exact bytes, byte-offset
                            selections, undo/redo with time-based grouping
               file.rs      load (UTF-8; else read-only), atomic save with conflict check
               markdown/    Obsidian-flavoured parser: block splitter + inline tokenizer
                            (wikilinks, embeds, ==highlight==, $math$, callouts, tables,
                            ^block-id, %%…%%); every node carries its source byte range
               index.rs     vault index: links, headings, tags, block ids, callouts,
                            backlinks, rename/merge plans; unique generation counter
               search.rs    full-text search, subset of Obsidian's query syntax
               sort.rs      file-list sort orders (alphanumeric, case/accent-insensitive)
crates/app     tephra — the GPUI app
               editor/      live-preview editor on gpui-kit's editor-host API: a projection
                            maps document+selection to a BlockNode tree whose text leaves
                            hold Text / Hidden / Atom segments; constructs intersecting the
                            caret reveal their source; blocks are cached by (text, reveal),
                            so a keystroke rebuilds one block
               math.rs      RaTeX (native KaTeX port) rasterised at device scale, cached
                            (capped at 1024); lazy MathJax-in-QuickJS fallback for the few
                            formulas RaTeX rejects, bounded to ~3 ms per frame
               plugins/     the plugin system + all bundled features (see below)
               export/      PDF export: note → Typst markup → PDF in a separate worker
                            process (`--export-worker`), MathJax SVG for math, preview
                            pages rasterised to PNG
               theme.rs     themes/snippets: GPUI has no CSS engine, so only Obsidian's CSS
                            *variables* apply (global, class-scoped, --callout-color)
tools/js-bundles   builds the embedded MathJax-SVG and headless-CM6 bundles (Node, esbuild)
tools/perf         `make perf` harness: proctree.py (process-tree memory), run_perf.sh,
                   obsidian_run.mjs (Obsidian baseline, isolated profile)
tools/winshot      KWin screenshot tool used by the scripted UI tests
tests/ui/          scripted UI tests: they drive the real binary with TEPHRA_SCRIPT=<file>
packaging/         Arch / Debian / Windows packages (see packaging/README.md)
themes/            the bundled Tephra theme (dark/light variants follow the base scheme)
```

**Plugin system.** Every feature — including the "core" ones like search, backlinks and the
outline — is a bundled plugin on one API (`crates/app/src/plugins/api.rs`): a `Plugin` trait
with `on_load`/`on_unload`, a `Registrar` that records every registration so unloading is
total, and registration points for markdown post-processors, code-block renderers (with
nested Markdown rendering and a Typst export hook), callout renderers, editor hooks,
commands/hotkeys, sidebar and tab views, caret popups, context menus, settings tabs and
status-bar items. Toggling a plugin applies without a restart. Plugins get read-only access
to the index and vault APIs; all writes go through the core. Settings live in
`.tephra/plugins/<id>.json`. Design for third-party plugins later; sandboxing is out of
scope.

**Layout/state.** One vault per process; opening another vault starts a new window.
Open tabs, pane splits and view modes are saved to `.tephra/workspace.json` (Obsidian's
shape) and restored on start. UI fonts are embedded in the binary so rendering is identical
on every machine.

## Working on the repo

- **Build:** Rust is pinned by `rust-toolchain.toml` (via rustup). Node.js + npm are needed
  once for `make bundles` (the embedded JS bundles are gitignored). Then `make build`
  (release build of the app plus `winshot`). Plain `cargo build --release -p tephra` works
  once the bundles exist. `make help` lists all targets.
- **Test:** `make test` — `cargo test --release --workspace` (fixture tests skip when no
  fixture is present). `make test-all` additionally runs `export-check` (every fixture note
  converts to Typst and compiles to PDF) and `ui-smoke`, the scripted UI tests, which need a
  display. The UI test list is the `for t in …` loop in the `ui-smoke` Makefile target.
- **Measure:** `make perf` — idle memory (RSS/PSS/USS summed over the whole process tree),
  cold start, typing, search and indexing; raw JSONL in `reports/` (gitignored). Set
  `VAULT=<vault>` to measure on a real vault; without it a synthetic math-heavy benchmark
  vault is generated. `make perf-obsidian` (or `OBSIDIAN=1 make perf`) adds the Obsidian
  baseline on the same vault via an isolated profile. `TEPHRA_BENCH=1` runs the in-app
  typing benchmark.
- **Package:** `make arch` / `make deb` / `make windows` / `make dist` — outputs in `dist/`,
  see `packaging/README.md` (Debian and Windows build in containers via podman).
- **Style:** small commits with clear messages; run the tests before every commit. Comments
  explain why, not what. When a vault's real content and an assumption in the code
  disagree, the vault (Obsidian compatibility) wins. Keep this file current.

## Performance targets and current numbers

Reference hardware: **Ryzen 7 7735HS + RTX 4050 Laptop GPU (6 GB), 16 GB RAM, Arch Linux,
KDE Wayland**. Baseline: **Obsidian 1.15.1 (Electron 43)** on the same machine, isolated
profile, vault open with the heaviest math note displayed. Memory is summed over the whole
process tree (RSS double-counts shared libraries; PSS splits them; USS is private only).
Cold start is exec → note visible and interactive, warm page cache.

Targets (measured with `make perf`):

| Metric | Target |
|---|---:|
| Idle memory, vault open, heaviest math note | ≤ 50% of Obsidian's RSS (≤ ~475 MiB RSS, ≤ ~300 MiB PSS) |
| Cold start to interactive | < 1.5 s |
| Typing frame work per keystroke in a ~20 KB math-heavy note (~200 formulas, 13 images) | < 16 ms |
| Full-text search across the vault | < 100 ms |
| Full vault index (then incremental) | < 2 s |

Current numbers (latest `make perf` run on the reference machine, math-heavy note open):

| Metric | Obsidian baseline | Tephra | Target |
|---|---:|---:|---:|
| Idle RSS / PSS | 938–979 / 590–631 MiB | **282 / 248 MiB** (~30% / ~42% of Obsidian RSS/PSS) | ≤ ~475 / ~300 MiB ✓ |
| Cold start | ~2–4 s | **0.43 s** | < 1.5 s ✓ |
| Typing, plain text p50 / p95 / max | — | **7.1 / 7.7 / 8.1 ms** | < 16 ms ✓ |
| Typing inside a formula p50 / max | — | **10.2 / 13.8 ms** | < 16 ms ✓ |
| Search p50 / max | — | **0.3 / 8.1 ms** | < 100 ms ✓ |
| Full index | — | **42 ms** | < 2 s ✓ |

Related measurements on the same machine: graph view with attachments (1,630 nodes /
1,285 edges) builds in 1.3 ms and settles in ~2.5 s (Barnes–Hut, 2–3 ticks per frame only
while settling); PDF export of the math-heavy note (12 pages, ~200 formulas) takes ~1.5 s
and runs in the worker process, so the app's memory does not move.
