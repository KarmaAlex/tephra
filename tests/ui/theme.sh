#!/usr/bin/env bash
# Themes and snippets: the base colour scheme from .obsidilight/app.json (light, then dark);
# a theme's variables apply; a note-specific snippet (.NoBorder) applies to notes listing the
# class in cssclasses even though the snippet is not enabled; rules that aren't variables are
# counted as ignored.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian/snippets" "$TMP/.obsidian/themes/Hue" "$TMP/.obsidilight"
printf '.NoBorder {\n--table-border-width: 0px;\n--table-white-space: normal;\n}\n' > "$TMP/.obsidian/snippets/table_border.css"
printf '.theme-dark { --accent-h: 120; }\n.markdown-preview-view h1 { text-transform: uppercase; }\n' > "$TMP/.obsidian/themes/Hue/theme.css"
echo '{"cssTheme": "Hue"}' > "$TMP/.obsidian/appearance.json"
printf '| a | b |\n|---|---|\n| 1 | 2 |\n' > "$TMP/T.md"
printf -- '---\ncssclasses: NoBorder\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n' > "$TMP/N.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
theme
ws-open N.md
wait 200
theme
quit
SCRIPT
run() { OBSIDILIGHT_SCRIPT="$TMP/s.script" timeout 30 target/release/obsidilight "$TMP/T.md" | grep -E "^THEME"; }
echo '{"baseTheme": "light"}' > "$TMP/.obsidilight/app.json"
OUT=$(run); echo "$OUT"
fail() { echo "ui-theme: $1"; exit 1; }
grep -qF 'THEME dark=false bg=#ffffff classes=[] table-border=Some(1.0) accent=Some("hsl(258, 88%, 66%)")' <<<"$OUT" || fail "light, plain note"
grep -qF 'THEME dark=false bg=#ffffff classes=["NoBorder"] table-border=Some(0.0)' <<<"$OUT" || fail "NoBorder note"
grep -qF 'THEME-FILE table_border theme=false on=false global=0 class=2 ["NoBorder"] ignored=0' <<<"$OUT" || fail "snippet report"
grep -qF 'THEME-FILE Hue theme=true on=true global=1 class=0 [] ignored=1' <<<"$OUT" || fail "theme report"
echo '{"baseTheme": "dark"}' > "$TMP/.obsidilight/app.json"
OUT=$(run); echo "$OUT"
grep -qF 'THEME dark=true bg=#1e1e1e classes=[] table-border=Some(1.0) accent=Some("hsl(120, 88%, 66%)")' <<<"$OUT" || fail "dark, with the theme's accent"
echo "ui-theme: OK"
