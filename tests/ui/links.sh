#!/usr/bin/env bash
# Plain click on a link must not open it; Ctrl+click must. Needs a display.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf 'vedi [[empty]] e [sito](https://example.com) fine\n\naltro\n' > "$TMP/links.md"
: > "$TMP/empty.md"
OUT=$(OBSIDILIGHT_SCRIPT=tests/ui/links.script timeout 30 target/release/obsidilight "$TMP/links.md")
echo "$OUT" | grep -E "STATE|OPEN"
[[ $(echo "$OUT" | grep -c OPEN_) == 2 ]] || { echo "links: expected exactly 2 opens (the Ctrl+clicks)"; exit 1; }
echo "$OUT" | grep -q "OPEN_NOTE .*empty.md" && echo "$OUT" | grep -q "OPEN_URL https://example.com"
echo "$OUT" | grep "STATE" | head -1 | grep -q "sel=8..8" || { echo "links: plain click should place the caret in the link"; exit 1; }
echo "ui-links: OK"
