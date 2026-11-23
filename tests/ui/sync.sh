#!/usr/bin/env bash
# Live sync: a note open in two panes shows every keystroke in both, before any save; undo in
# a pane undoes only that pane's own typing; one save leaves both panes clean.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'alpha\n' > "$TMP/A.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
key ctrl-p
type split right
key enter
wait 400
key ctrl-end
type XY
wait 100
ws-texts
key ctrl-alt-right
wait 100
key ctrl-home
type Q
wait 100
ws-texts
key ctrl-z
wait 100
ws-texts
wait 1600
ws-texts
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/s.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^TEXTS")
echo "$OUT"
# in order; the "d" (unsaved) marks depend on autosave timing, except the last line
GOT=$(sed 's/d"/"/g' <<<"$OUT")
WANT='TEXTS "alpha\nXY" | "alpha\nXY"
TEXTS "Qalpha\nXY" | "Qalpha\nXY"
TEXTS "alpha\nXY" | "alpha\nXY"
TEXTS "alpha\nXY" | "alpha\nXY"'
[ "$GOT" = "$WANT" ] || { echo "ui-sync: expected:"; echo "$WANT"; exit 1; }
[ "$(tail -1 <<<"$OUT")" = 'TEXTS "alpha\nXY" | "alpha\nXY"' ] || { echo "ui-sync: both panes should be saved"; exit 1; }
[ "$(cat "$TMP/A.md")" = $'alpha\nXY' ] || { echo "ui-sync: file on disk: $(cat "$TMP/A.md")"; exit 1; }
echo "ui-sync: OK"
