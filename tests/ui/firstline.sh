#!/usr/bin/env bash
# Regression: a note whose first line's leaf key collided with a plugin-rendered callout body
# (the nested Markdown render reused leaf key 0 and overwrote the editor's leaf geometry).
# Symptoms: the first line was placed hundreds of px down, so it was not clickable, Up from
# line 2 jumped elsewhere, and editing the first line scrolled the viewport down.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
{ printf '## Title\n\n'
  for i in $(seq 1 30); do printf 'line %03d lorem ipsum dolor sit amet\n' "$i"; done
  printf '\n>[!algorithm|lines=true] Greedy\n> $(X_1, X_2) \\leftarrow (\\emptyset, \\emptyset)$\n> for every $v$ do\n\n'
  for i in $(seq 31 60); do printf 'line %03d lorem ipsum dolor sit amet\n' "$i"; done
} > "$TMP/n.md"
cat > "$TMP/t.script" <<'SCRIPT'
wait 900
pos 0
state
type X
wait 400
pos 0
state
key up
wait 200
state
clickat 0
wait 200
state
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/t.script" timeout 40 target/release/obsidilight "$TMP/n.md" | grep -E "^(STATE|POS|CLICKAT)")
echo "$OUT"
fail() { echo "ui-firstline: $1"; exit 1; }
mapfile -t POS <<<"$(grep -E '^POS 0 ' <<<"$OUT")"
mapfile -t ST <<<"$(grep -E '^STATE ' <<<"$OUT")"
[ "${#POS[@]}" -ge 2 ] && [ "${#ST[@]}" -ge 4 ] || fail "not enough output"
# the first line is drawn near the top of the editor, not hundreds of px down
for p in "${POS[@]}"; do
  Y=$(awk '{print $4}' <<<"$p")
  [ "$Y" -lt 200 ] || fail "first line drawn at y=$Y ('$p'), not at the top"
done
# editing the first line does not scroll the viewport (a few px of restyle is fine)
Y1=$(awk '{print $4}' <<<"${POS[0]}"); Y2=$(awk '{print $4}' <<<"${POS[1]}")
D=$(( Y2 - Y1 )); [ "$D" -lt 0 ] && D=$(( -D ))
[ "$D" -lt 30 ] || fail "typing in line 1 moved the viewport: y $Y1 -> $Y2"
# the caret stays on line 1 while editing it
[[ "${ST[0]}" =~ caret_px=\([0-9]+,([0-9]+) ]] && CY=${BASH_REMATCH[1]}
[ "${CY:-999}" -lt 200 ] || fail "caret at y=$CY on open, not at the top"
[[ "${ST[1]}" =~ sel=1\.\.1\ line=1 ]] || fail "typing did not land in line 1: ${ST[1]}"
# Up from line 1 stays on line 1
[[ "${ST[2]}" =~ sel=0\.\.0\ line=1 ]] || fail "Up from line 1 jumped: ${ST[2]}"
# clicking the first line places the caret at offset 0
[[ "${ST[3]}" =~ sel=0\.\.0\ line=1 ]] || fail "click on line 1 did not place the caret: ${ST[3]}"
echo "ui-firstline: OK"
