#!/usr/bin/env bash
# Runs tests/ui/smoke.script against a generated note and checks that the
# edit + undo + autosave cycle leaves the file byte-identical. Needs a display.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian" "$TMP/Images"
{
  echo "# Smoke note"
  echo
  for i in $(seq 1 120); do
    echo "Paragraph $i with inline math \$x_{$i} + \\frac{a}{b}\$."
    echo
  done
  echo '![[img.png]]'
} > "$TMP/note.md"
printf 'PNG' > "$TMP/Images/img.png"
cp "$TMP/note.md" "$TMP/orig.md"
TEPHRA_SCRIPT=tests/ui/smoke.script timeout 60 target/release/tephra "$TMP/note.md" | grep STATE
cmp "$TMP/orig.md" "$TMP/note.md" && echo "ui-smoke: OK (byte-identical after edit + undo + autosave)"
