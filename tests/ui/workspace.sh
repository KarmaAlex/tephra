#!/usr/bin/env bash
# What the window remembers between runs (`.tephra/workspace.json`, our own file next to
# Obsidian's read-only `.obsidian/workspace.json`): the notes open in each pane, their tabs and
# the active one, the panes' arrangement, which pane had the focus and each note's view mode.
# A note deleted while the app was closed is not reopened. Needs a display.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf '# A\nalpha\n' > "$TMP/A.md"
printf '# B\nbeta\n' > "$TMP/B.md"
printf '# C\ngamma\n' > "$TMP/C.md"

# first run: two notes side by side, B in source mode, focus on the right pane
cat > "$TMP/save.script" <<'SCRIPT'
wait 800
ws-open A.md
wait 300
key ctrl-p
type split right
key enter
wait 400
ws-open B.md
wait 300
key ctrl-shift-e
wait 300
ws-panes
ws-modes
quit
SCRIPT
FIRST=$(TEPHRA_SCRIPT="$TMP/save.script" timeout 30 target/release/tephra "$TMP" | grep -E "^(PANES|MODES)")
echo "$FIRST"

# second run: the same, without touching anything
cat > "$TMP/load.script" <<'SCRIPT'
wait 800
ws-panes
ws-modes
quit
SCRIPT
SECOND=$(TEPHRA_SCRIPT="$TMP/load.script" timeout 30 target/release/tephra "$TMP" | grep -E "^(PANES|MODES)")
echo "$SECOND"
[ "$FIRST" = "$SECOND" ] || { echo "ui-workspace: the window did not come back as it was"; exit 1; }
grep -qxF 'PANES ["A.md"] | *["B.md"]' <<<"$SECOND" || { echo "ui-workspace: wrong panes"; exit 1; }
grep -qxF 'MODES A.md:live | *B.md:source' <<<"$SECOND" || { echo "ui-workspace: wrong modes"; exit 1; }
grep -q '"type": "split"' "$TMP/.tephra/workspace.json" || { echo "ui-workspace: no split saved"; exit 1; }
grep -q '"mode": "source"' "$TMP/.tephra/workspace.json" || { echo "ui-workspace: view mode not saved"; exit 1; }

# a note deleted while the app was shut is dropped, and its pane goes with it
rm "$TMP/B.md"
OUT=$(TEPHRA_SCRIPT="$TMP/load.script" timeout 30 target/release/tephra "$TMP" | grep -E "^PANES")
echo "$OUT"
grep -qxF 'PANES *["A.md"]' <<<"$OUT" || { echo "ui-workspace: the deleted note's pane is still there"; exit 1; }

# a tab closed before the app shut stays closed
cat > "$TMP/close.script" <<'SCRIPT'
wait 800
ws-open C.md new
wait 300
ws-panes
key ctrl-w
wait 300
ws-panes
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/close.script" timeout 30 target/release/tephra "$TMP" | grep -E "^PANES")
echo "$OUT"
grep -qxF 'PANES *["A.md", "C.md"]' <<<"$OUT" || { echo "ui-workspace: C did not open in a new tab"; exit 1; }
OUT=$(TEPHRA_SCRIPT="$TMP/load.script" timeout 30 target/release/tephra "$TMP" | grep -E "^PANES")
echo "$OUT"
grep -qxF 'PANES *["A.md"]' <<<"$OUT" || { echo "ui-workspace: the closed tab came back"; exit 1; }

# a note named on the command line opens on top of the remembered tabs
OUT=$(TEPHRA_SCRIPT="$TMP/load.script" timeout 30 target/release/tephra "$TMP/C.md" | grep -E "^(PANES|MODES)")
echo "$OUT"
grep -qxF 'PANES *["A.md", "C.md"]' <<<"$OUT" || { echo "ui-workspace: the note on the command line did not open next to the remembered one"; exit 1; }
grep -qxF 'MODES *C.md:live' <<<"$OUT" || { echo "ui-workspace: the note on the command line is not the active tab"; exit 1; }

echo "ui-workspace: OK (open notes, panes and view modes come back)"
