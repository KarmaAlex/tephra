#!/usr/bin/env bash
# Focus mode: only the focused pane's note (no ribbon, sidebars, tab bars, other panes); the
# same command restores everything. (Fullscreen focus mode also toggles the window's full
# screen; not run here so the test never takes over the screen.)
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf '# A\nalpha\n' > "$TMP/A.md"
printf '# B\nbeta\n' > "$TMP/B.md"
cat > "$TMP/s.script" <<SCRIPT
wait 800
key ctrl-p
type split right
key enter
wait 300
ws-open B.md
key ctrl-p
type toggle focus mode
key enter
wait 300
${SHOT:+shot $SHOT}
ws-panes
key ctrl-p
type toggle focus mode
key enter
wait 300
ws-panes
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/A.md" | grep -E "^(FOCUS|PANES)")
echo "$OUT"
WANT='FOCUS true fullscreen=false
PANES ["A.md"] | *["B.md"]
FOCUS false fullscreen=false
PANES ["A.md"] | *["B.md"]'
[ "$OUT" = "$WANT" ] || { echo "ui-focus: expected:"; echo "$WANT"; exit 1; }
echo "ui-focus: OK"
