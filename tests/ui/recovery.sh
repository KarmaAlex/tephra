#!/usr/bin/env bash
# File recovery: editing keeps the text from before the first edit and the edit; a change made
# outside the app keeps the text it replaced; the history dialog lists the snapshots and
# restores the newest one.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf 'original\n' > "$TMP/A.md"
cat > "$TMP/s.script" <<SCRIPT
wait 800
key ctrl-end
type edited
wait 1500
type X
wait 1500
wait 2000
key ctrl-p
type open local history
key enter
wait 500
${SHOT:+shot $SHOT}
click 982 623
wait 1500
quit
SCRIPT
# Syncthing-like outside change while the app runs (after the autosave)
( sleep 5.0; printf 'from the phone\n' > "$TMP/A.md" ) &
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/A.md" | grep -E "^(RECOVERY|SHOT)")
echo "$OUT"
wait
D=$(ls -d "$TMP"/.tephra/test-data/recovery/*/*/)
TEXTS=$(for f in $(ls "$D"*.md | sort); do printf '[%s]' "$(cat "$f")"; done)
echo "$TEXTS"
# before the first edit, the first edit; the second edit (within the interval) only because the
# outside change replaced it; the text a restore replaced
[ "$TEXTS" = $'[original][original\nedited][original\neditedX][from the phone]' ] || { echo "ui-recovery: snapshots: $TEXTS"; exit 1; }
grep -qF "RECOVERY A.md 3" <<<"$OUT" || { echo "ui-recovery: history dialog"; exit 1; }
[ "$(cat "$TMP/A.md")" = $'original\neditedX' ] || { echo "ui-recovery: restore: $(cat "$TMP/A.md")"; exit 1; }
echo "ui-recovery: OK"
