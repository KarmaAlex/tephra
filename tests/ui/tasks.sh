#!/usr/bin/env bash
# Task lists in the running app: clicking a checkbox ticks it (saved to disk), the caret can sit
# and move on task lines, and typing there edits the text. Synthetic note in a temporary vault.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf '# Tasks\n- [ ] uno\n- [x] due\nfine\n' > "$TMP/n.md"
UNO=$(python3 -c "print(open('$TMP/n.md').read().find('uno'))")
printf 'wait 800\nkey ctrl-end\nwait 300\npos %s\nquit\n' "$UNO" > "$TMP/p.script"
P=$(OBSIDILIGHT_SCRIPT="$TMP/p.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep POS)
X=$(echo "$P" | awk '{print $3-16}'); Y=$(echo "$P" | awk '{print $4}')
cat > "$TMP/a.script" <<SCRIPT
wait 800
key ctrl-end
wait 300
click $X $Y
wait 1500
key ctrl-home
key down
state
key end
type !
state
key down
state
wait 1500
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/a.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep -E "^(TASK|STATE)")
echo "$OUT"
fail() { echo "ui-tasks: $1"; exit 1; }
grep -qx 'TASK \[x\]' <<<"$OUT" || fail "clicking the checkbox did not tick it"
[ "$(grep -c 'caret_px=none' <<<"$OUT")" = 0 ] || fail "the caret disappeared on a task line"
grep -qF 'text="- [x] uno!|"' <<<"$OUT" || fail "typing on a task line"
grep -qF 'text="- [x] due|"' <<<"$OUT" || fail "moving down onto the next task"
[ "$(sed -n 2p "$TMP/n.md")" = "- [x] uno!" ] || fail "file not saved: $(sed -n 2p "$TMP/n.md")"
echo "ui-tasks: OK"
