#!/usr/bin/env bash
# Editing toolbar commands in the running app: Ctrl+B wraps the selection in **, a heading
# command, a bullet list, and Ctrl+B again unwraps. Uses your toolbar layout if the fixture
# has one (the commands don't depend on it).
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'word here\nnext\n' > "$TMP/n.md"
cat > "$TMP/t.script" <<'SCRIPT'
wait 800
key ctrl-home
key shift-ctrl-right
key ctrl-b
wait 200
state
key ctrl-p
type heading 2
key enter
wait 300
state
key down
key ctrl-p
type bullet list
key enter
wait 300
state
key up
key end
key shift-home
key ctrl-p
type clear text formatting
key enter
wait 1500
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/t.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep -E "^STATE")
echo "$OUT"
grep -qF 'text="**word|** here"' <<<"$OUT" || { echo "ui-toolbar: bold"; exit 1; }
grep -qF 'text="## **word** here|"' <<<"$OUT" || { echo "ui-toolbar: heading"; exit 1; }
grep -qF 'text="- next|"' <<<"$OUT" || { echo "ui-toolbar: bullet list"; exit 1; }
[ "$(head -1 "$TMP/n.md")" = "## word here" ] || { echo "ui-toolbar: clear formatting / save: $(head -1 "$TMP/n.md")"; exit 1; }
echo "ui-toolbar: OK"
