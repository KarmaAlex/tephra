#!/usr/bin/env bash
# Horizontal blocks in the running app: three columns side by side (`---` inside fenced code
# doesn't split), dragging a divider resizes the column on its left without revealing the
# block's source, the width is saved in Obsidian's key format and comes back on reopening.
# Synthetic note in a temporary vault.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.tephra"
echo '{"enabled": ["horizontal-blocks"], "known": ["horizontal-blocks"]}' > "$TMP/.tephra/plugins.json"
cat > "$TMP/n.md" <<'NOTE'
# Title

````hblock
### Left
text
---
```
---
```
---
### Right
more
````

after
NOTE
run() { printf "$1" > "$TMP/s.script"; TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/n.md"; }
OUT=$(run 'wait 1000\nbounds hblock-col:0\nbounds hblock-col:2\nbounds hblock-divider:0\nquit\n' | grep BOUNDS)
echo "$OUT"
read -r _ _ X0 _ W0 _ <<<"$(grep 'col:0' <<<"$OUT")"
grep -q 'col:2 none' <<<"$OUT" && { echo "ui-hblocks: expected three columns"; exit 1; }
read -r _ _ DX DY _ DH <<<"$(grep 'divider:0' <<<"$OUT")"
X=$((DX + 5)); Y=$((DY + DH / 2))
OUT=$(run "wait 1000\ndrag $X $Y $((X + 120)) $Y\nwait 300\nbounds hblock-col:0\nstate\nquit\n")
echo "$OUT" | grep -E "BOUNDS|STATE"
read -r _ _ _ _ W1 _ <<<"$(grep BOUNDS <<<"$OUT")"
[ $((W1 - W0)) -ge 115 ] && [ $((W1 - W0)) -le 125 ] || { echo "ui-hblocks: column 0 went from $W0 to $W1 px, expected +120"; exit 1; }
grep -q 'sel=0\.\.0' <<<"$OUT" || { echo "ui-hblocks: the drag moved the caret into the block"; exit 1; }
grep -qE '"horizontal-block-layout-[0-9a-f]{16}"' "$TMP/.tephra/horizontal-blocks.json" && grep -q '"width-0"' "$TMP/.tephra/horizontal-blocks.json" || { echo "ui-hblocks: width not saved"; cat "$TMP/.tephra/horizontal-blocks.json"; exit 1; }
OUT=$(run 'wait 1000\nbounds hblock-col:0\nquit\n' | grep BOUNDS)
read -r _ _ _ _ W2 _ <<<"$OUT"
[ "$W2" = "$W1" ] || { echo "ui-hblocks: reopened at $W2 px, saved $W1"; exit 1; }
echo "ui-hblocks: OK"
