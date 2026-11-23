#!/usr/bin/env bash
# Hotkeys from .obsidilight/hotkeys.json: the quick switcher moved from Ctrl+O to Ctrl+A (which
# the editor would use for Select all): Ctrl+A opens it, Ctrl+O no longer does.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidilight"
printf '# Note\ntext\n' > "$TMP/n.md"
echo '{"switcher:open": ["ctrl-a"]}' > "$TMP/.obsidilight/hotkeys.json"
printf 'wait 800\nkey ctrl-o\nwait 300\nstate\nkey ctrl-a\nwait 300\nkey escape\nwait 200\nstate\nquit\n' > "$TMP/h.script"
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/h.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep -E "^(PICKER|STATE)")
echo "$OUT"
[ "$(grep -c '^PICKER' <<<"$OUT")" -ge 1 ] || { echo "ui-hotkeys: Ctrl+A did not open the switcher"; exit 1; }
[ "$(head -1 <<<"$OUT" | cut -c1-5)" = "STATE" ] || { echo "ui-hotkeys: Ctrl+O still opened it"; exit 1; }
grep -q 'STATE sel=0\.\.0 ' <<<"$OUT" && ! grep -q 'sel=0\.\.14' <<<"$OUT" || { echo "ui-hotkeys: Ctrl+A also selected all"; exit 1; }
echo "ui-hotkeys: OK"
