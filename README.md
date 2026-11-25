
<p align="center"><img src="assets/tephra-banner.png" alt="Tephra banner" height="50%"></p>

<p align="center">
  <em>A lightweight, Obsidian-compatible notes app for the desktop</em>
</p>

Tephra is a native notes app (Rust + GPUI) that opens your
existing Obsidian vaults in place and reads `.obsidian/`, keeping its own settings
in `.tephra/` inside the vault. It has a live-preview Markdown editor, typeset math (RaTeX, MathJax fallback), and the features Obsidian gets from plugins — search, graph,
backlinks, daily notes, templates, spellcheck, PDF export and more — built in as toggleable
plugins.

## Build

Requirements:

- Rust (pinned by `rust-toolchain.toml`, installed via rustup)
- Node.js + npm — only once, to build the embedded JS bundles (`make bundles`)

```sh
make build          # release build of `tephra` (+ winshot, the UI tests' screenshot tool)
target/release/tephra <vault-folder | note.md>
```

`make build` builds the JS bundles first if they are missing (they are gitignored). Plain
`cargo build --release -p tephra` works too once the bundles exist. `make help` lists all
targets.

## Test

```sh
make test           # unit + fixture tests
make test-all       # + PDF export of every fixture note + scripted UI tests (need a display)
```

Scripted UI tests drive the real binary with `TEPHRA_SCRIPT=<file>`; `TEPHRA_BENCH=1` runs the
typing benchmark.

## Packaging

One command per platform, output in `dist/` (see `packaging/README.md`):

| Command | Output |
|---|---|
| `make arch` | `dist/tephra-<ver>-<rel>-x86_64.pkg.tar.zst` (native makepkg) |
| `make deb` | `dist/tephra_<ver>-1_amd64.deb` (Debian 12 container) |
| `make windows` | `dist/tephra-<ver>-windows-x86_64.zip` (cross-compiled in a container) |
| `make dist` | all three |

## Themes

The default look follows Obsidian's own light/dark variables. One built-in theme ships with the
app as a secondary choice (Settings → Appearance): **Tephra**, a volcanic-ash palette (warm
basalt greys and ember accent in dark mode, warm paper and dark ember in light mode). It is a
single theme with two variants — the dark or light variant is picked by the base color scheme
setting (system/light/dark). Its source lives in `themes/Tephra/theme.css`; copy the folder into
the app's themes folder to install it as an installed theme (an installed theme of the same name
overrides the bundled one).

Themes are per user, not per vault: any Obsidian theme installed in `<config dir>/themes/`
(`~/.config/tephra/themes`, `%APPDATA%\tephra\themes` on Windows) as `<name>/theme.css` (or
`<name>.css`) is offered in Settings → Appearance and applies to every vault; a vault's
`.obsidian/themes` is not read. Only a theme's CSS variables apply (colours, fonts, sizes).
CSS snippets still come from the vault (`.obsidian/snippets`), and only their variables apply
there too.

## Layout

```
crates/core    tephra-core — platform-agnostic core: document buffer, file I/O, Markdown parser
crates/app     tephra      — the GPUI app: editor, plugins, themes, vault UI
tools/winshot              — KWin screenshot tool used by the UI tests
tools/js-bundles           — builds the embedded MathJax/CM6 bundles (Node.js)
themes/                    — the built-in Tephra themes (dark & light variants)
assets/                    — logo (SVG) and banner
packaging/                 — Arch / Debian / Windows packages
tests/ui/                  — scripted UI tests
```

## Configuration

Per vault: `<vault>/.tephra/` (app settings, plugin settings, workspace layout). Per user:
`~/.config/tephra/` (`%APPDATA%\tephra` on Windows): the vault list and `themes/`. Environment:
`TEPHRA_CONFIG_DIR`, `TEPHRA_DATA_DIR`, `TEPHRA_LOG`, `TEPHRA_LOG_LEVEL`, `TEPHRA_SCRIPT`,
`TEPHRA_BENCH`.

## License

MIT — see [LICENSE](LICENSE). The embedded UI fonts (DejaVu, IBM Plex Sans) carry their own
licenses in `crates/app/assets/fonts/`.
