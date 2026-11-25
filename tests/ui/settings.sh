#!/usr/bin/env bash
# Settings layout: every page puts its settings in the same two columns — the title and its
# description on the left, the control in one fixed-width column on the right — so every control
# (switch, dropdown, text box, number box, hotkey chips) starts and ends at the same x, and none of
# them can be pushed past the page's edge or overlap its description. Each kind of control also
# writes what it is given: the switch, the dropdown, the text box and the number box's stepper.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'nothing yet\n' > "$TMP/a.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
resize 1280 860
wait 400
key ctrl-,
wait 2000
# Editor: switches, each in the field column, the description to their left
bounds settings:auto-pair-brackets:field
bounds settings:auto-pair-brackets:control
bounds settings:centre-display-math:field
bounds settings:centre-display-math:control
clickk settings:auto-pair-brackets:control
wait 600
# Appearance: dropdowns, as wide as the column
click 268 225
wait 1200
bounds settings:base-color-scheme:field
bounds settings:interface-scale:field
bounds settings:theme:field
clickk settings:base-color-scheme:field
wait 400
key down
key down
key enter
wait 600
# Hotkeys: the chips right-aligned in the very same column
click 268 297
wait 1200
bounds hotkeys:app:save:field
bounds hotkeys:app:open-settings:field
# Templates: text boxes
click 268 477
wait 1200
bounds settings:template-folder:field
bounds settings:date-format:field
clickk settings:template-folder:field
wait 300
type tmpl
wait 400
# Graph view: a number box, stepped up with its +
click 268 621
wait 1200
bounds settings:center-force:field
clickk settings:center-force:field 84 0
wait 500
quit
SCRIPT
run() { TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP" | grep -E "^BOUNDS"; }
OUT=$(run); echo "$OUT"
fail() { echo "ui-settings: $1"; exit 1; }

# every row measured (each page was really open: a row that isn't on screen reads `none`)
grep -q ':field none' <<<"$OUT" && fail "a row was not on screen: $(grep ':field none' <<<"$OUT" | tr '\n' ' ')"
ROWS=$(grep -cE 'BOUNDS .*:field ' <<<"$OUT") || true
[ "$ROWS" -ge 10 ] || fail "only $ROWS rows were drawn"

# one column for every page: the same x and the same width everywhere
X=$(awk '/:field /{print $3}' <<<"$OUT" | sort -u)
W=$(awk '/:field /{print $5}' <<<"$OUT" | sort -u)
[ "$(wc -l <<<"$X")" -eq 1 ] || fail "the field columns start at different x: $(tr '\n' ' ' <<<"$X")"
[ "$(wc -l <<<"$W")" -eq 1 ] || fail "the field columns differ in width: $(tr '\n' ' ' <<<"$W")"
RIGHT=$((X + W))

# a control ends where its column ends, so nothing overlaps the description or leaves the page
while read -r key control_x _ control_w _; do
    [ "$((control_x + control_w))" -eq "$RIGHT" ] || fail "$key ends at $((control_x + control_w)), the column at $RIGHT"
done < <(grep -E 'BOUNDS .*:control ' <<<"$OUT" | sed 's/^BOUNDS //')

# and each kind of control saves what it was given
grep -q '"autoPairBrackets": false' "$TMP/.tephra/app.json" || fail "the switch did not save"
grep -q '"baseTheme": "light"' "$TMP/.tephra/app.json" || fail "the dropdown did not save"
grep -q '"folder": "tmpl"' "$TMP/.tephra/plugins/templates.json" || fail "the text box did not save"
grep -q '"centerStrength": 1.52' "$TMP/.tephra/plugins/graph.json" || fail "the number box's + did not save"
echo "ui-settings: OK"
