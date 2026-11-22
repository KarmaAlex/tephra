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
echo "$OUT" | grep -E "STATE|OPEN|TABS"
echo "$OUT" | grep "STATE" | head -1 | grep -q "sel=9..9" || { echo "links: plain click should place the caret in the link"; exit 1; }
echo "$OUT" | grep "TABS" | head -1 | grep -q '\["links.md"\]' || { echo "links: plain click must not open anything"; exit 1; }
echo "$OUT" | grep -q "OPEN_URL https://example.com" || { echo "links: Ctrl+click on the URL should open it"; exit 1; }
echo "$OUT" | grep "TABS" | tail -1 | grep -q '\["empty.md"\]' || { echo "links: Ctrl+click on [[empty]] should open empty.md in the tab"; exit 1; }
# Ctrl+click on a link to a missing note creates it, and the file tree shows it
printf '[[nuova]]\n' > "$TMP/crea.md"
printf 'wait 400\nclickat 3 ctrl\nwait 500\nws-tree\nws-tabs\nquit\n' > "$TMP/crea.script"
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/crea.script" timeout 30 target/release/obsidilight "$TMP/crea.md")
echo "$OUT" | grep -E "TREE|TABS"
echo "$OUT" | grep "TREE" | grep -q '"nuova.md"' || { echo "links: the created note should appear in the tree"; exit 1; }
echo "ui-links: OK"
