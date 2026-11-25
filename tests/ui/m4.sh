#!/usr/bin/env bash
# M4 in the running app, on a temporary vault: search, quick switcher, command palette,
# backlinks, outgoing links, bookmarks (and bookmarks following a rename), tags -> search.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian" "$TMP/Corsi"
printf '# Alfa\n## Sez\nTesto [[Beta]] e ![[img.png]] e [[Manca]] #corso/algo\n' > "$TMP/Alfa.md"
printf 'Vedi [[Alfa#Sez]].\nAlfa senza link.\n#corso\n' > "$TMP/Corsi/Beta.md"
: > "$TMP/img.png"
cat > "$TMP/m4.script" <<'SCRIPT'
wait 600
key ctrl-shift-f
wait 200
type senza
wait 300
key ctrl-p
wait 200
type show backlinks
wait 200
key enter
wait 400
key ctrl-p
wait 200
type show outgoing
wait 200
key enter
wait 400
key ctrl-p
wait 200
type bookmark current file
wait 200
key enter
wait 400
key ctrl-o
wait 200
type beta
wait 200
key enter
wait 400
ws-tabs
key ctrl-p
wait 200
type show backlinks
wait 200
key enter
wait 400
ws-rename Alfa.md|Corsi/Alfa nuova.md
wait 400
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/m4.script" timeout 40 target/release/tephra "$TMP/Alfa.md" | grep -E "^(SEARCH|COMMAND|BACKLINKS|OUTGOING|BOOKMARK|TABS|RENAME)")
echo "$OUT"
fail() { echo "ui-m4: $1"; exit 1; }
grep -qF 'SEARCH "senza" files=1 lines=1' <<<"$OUT" || fail "search"
grep -qF 'COMMAND view:backlinks:backlinks' <<<"$OUT" || fail "palette did not run the view command"
grep -qF 'BACKLINKS Alfa.md linked=[("Corsi/Beta.md", 1)]' <<<"$OUT" || fail "backlinks"
grep -qF 'OUTGOING Alfa.md [("Corsi/Beta.md", true), ("img.png", true), ("Manca", false)]' <<<"$OUT" || fail "outgoing links"
grep -qF 'BOOKMARK Alfa.md true' <<<"$OUT" || fail "bookmark command"
grep -qF 'TABS active=0 ["Corsi/Beta.md"]' <<<"$OUT" || fail "quick switcher did not open Beta"
grep -qF 'BACKLINKS Corsi/Beta.md linked=[("Alfa.md", 1)]' <<<"$OUT" || fail "backlinks follow the active note"
grep -qF '"path": "Corsi/Alfa nuova.md"' "$TMP/.tephra/bookmarks.json" || fail "bookmark did not follow the rename"
grep -qF '"Corsi/Alfa nuova.md"' "$TMP/.tephra/workspace.json" || fail "recent files did not follow the rename"
echo "ui-m4: OK"
