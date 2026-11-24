#!/usr/bin/env bash
# Regression: caret placement.
# - Down inside an empty $$\n$$ block walks line by line (it used to jump to the document
#   end, because the step was half the caret height while code/math lines are taller).
# - Clicking past the right edge of a wrapped row keeps the caret on that row's end (it used
#   to move to the next row, because the wrap-point caret painted downstream).
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf '$$\n$$\n\nline below one\nline below two\nend\n' > "$TMP/math.md"
{ printf '## head\n'; for i in $(seq 1 60); do printf 'word%d ' "$i"; done; printf '\nshort line\nend\n'; } > "$TMP/wrap.md"

cat > "$TMP/m.script" <<'SCRIPT'
wait 900
key down
wait 200
state
key down
wait 200
state
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/m.script" timeout 40 target/release/obsidilight "$TMP/math.md" | grep -E "^STATE")
echo "$OUT"
fail() { echo "ui-caret: $1"; exit 1; }
mapfile -t ST <<<"$OUT"
[[ "${ST[0]}" =~ sel=3\.\.3\ line=2 ]] || fail "Down in a $$ block: expected sel=3..3 line=2, got ${ST[0]}"
[[ "${ST[1]}" =~ sel=6\.\.6\ line=3 ]] || fail "second Down: expected sel=6..6 line=3, got ${ST[1]}"

# y of the wrapped paragraph's first row (offset 8) and of a later row (offset 300)
cat > "$TMP/p.script" <<'SCRIPT'
wait 900
resize 1190 813
wait 300
pos 8
pos 300
quit
SCRIPT
POS=$(OBSIDILIGHT_SCRIPT="$TMP/p.script" timeout 40 target/release/obsidilight "$TMP/wrap.md" | grep -E "^POS")
echo "$POS"
Y1=$(awk '/^POS 8 /{print $4}' <<<"$POS")
Y2=$(awk '/^POS 300 /{print $4}' <<<"$POS")
[ -n "$Y1" ] && [ -n "$Y2" ] || fail "no positions"
[ "$Y2" -gt "$Y1" ] || fail "the paragraph did not wrap (y1=$Y1 y2=$Y2)"
cat > "$TMP/w.script" <<SCRIPT
wait 900
resize 1190 813
wait 300
click 1150 $Y1
wait 200
state
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/w.script" timeout 40 target/release/obsidilight "$TMP/wrap.md" | grep -E "^STATE")
echo "$OUT"
# the caret must stay on the first row of the wrapped paragraph (its y), not move to row 2
[[ "$OUT" =~ caret_px=\([0-9]+,([0-9]+) ]] || fail "no caret in output"
CY=${BASH_REMATCH[1]}
[ "$CY" -lt $(( Y2 - 5 )) ] || fail "click past the right edge moved the caret to row 2: caret y=$CY, row1 center=$Y1, row2 center=$Y2"
echo "ui-caret: OK"
