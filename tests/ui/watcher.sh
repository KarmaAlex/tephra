#!/usr/bin/env bash
# External changes while the app runs: a new note makes a link resolve, and an external rewrite
# of the open note reloads it. Needs a display.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf '[[b]]\n' > "$TMP/a.md"
TEPHRA_SCRIPT=tests/ui/watcher.script timeout 30 target/release/tephra "$TMP/a.md" > "$TMP/out.txt" &
APP=$!
sleep 2.4
printf 'nuova\n' > "$TMP/b.md"                                   # another device adds b.md
printf '[[b]] modificato altrove\n' > "$TMP/a.tmp" && mv "$TMP/a.tmp" "$TMP/a.md"   # and rewrites a.md (atomic, like Syncthing)
wait $APP
grep -E "RESOLVE|STATE" "$TMP/out.txt"
grep -q 'RESOLVE b -> None' "$TMP/out.txt" || { echo "watcher: b should start unresolved"; exit 1; }
grep -q 'RESOLVE b -> Some("b.md")' "$TMP/out.txt" || { echo "watcher: b.md created externally should resolve"; exit 1; }
grep -q 'modificato altrove' "$TMP/out.txt" || { echo "watcher: external rewrite should reload the open note"; exit 1; }
echo "ui-watcher: OK"
