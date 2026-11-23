#!/usr/bin/env bash
# Split panes: split right (same note), open another note in the new pane, split down, focus
# moves between panes, closing a pane's last tab closes the pane, renames reach every pane.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf '# A\nalpha\n' > "$TMP/A.md"
printf '# B\nbeta\n' > "$TMP/B.md"
cat > "$TMP/p.script" <<'SCRIPT'
wait 800
key ctrl-p
type split right
key enter
wait 400
ws-panes
ws-open B.md
wait 300
ws-panes
key ctrl-p
type split down
key enter
wait 400
ws-panes
key ctrl-alt-right
wait 200
ws-panes
ws-rename B.md|C.md
wait 300
ws-panes
key ctrl-w
wait 300
ws-panes
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/p.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^(PANES|RENAME)")
echo "$OUT"
expect() { grep -qxF "$1" <<<"$OUT" || { echo "ui-panes: expected: $1"; exit 1; }; }
expect 'PANES ["A.md"] | *["A.md"]'
expect 'PANES ["A.md"] | *["B.md"]'
expect 'PANES ["A.md"] | ["B.md"] | *["B.md"]'
expect 'PANES *["A.md"] | ["B.md"] | ["B.md"]'
expect 'PANES *["A.md"] | ["C.md"] | ["C.md"]'
expect 'PANES *["C.md"] | ["C.md"]'
echo "ui-panes: OK"
