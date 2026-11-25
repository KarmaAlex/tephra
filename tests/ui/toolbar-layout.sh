#!/usr/bin/env bash
# M8: the editing toolbar's layout edited in the app, and its colour pickers.
# - Settings opens at the toolbar's page from the command and from the bar's right-click menu.
# - Dragging rows reorders them and moves them into a menu; a command dragged in from the
#   list lands where it is dropped; + adds at the end; × removes. The bar follows at once.
# - Picking a colour colours the selection and becomes the button's colour; the button
#   then toggles it.
# - The layout and colours are saved in .tephra; .obsidian is never written.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian/plugins/editing-toolbar"
cat > "$TMP/.obsidian/plugins/editing-toolbar/data.json" <<'JSON'
{
  "cMenuFontColor": "#ff0000",
  "cMenuBackgroundColor": "#d3f8b6",
  "custom_fc1": "#D83931", "custom_fc2": "#DE7802", "custom_fc3": "#245BDB", "custom_fc4": "#6425D0", "custom_fc5": "#646A73",
  "menuCommands": [
    {"id": "editing-toolbar:toggle-bold", "name": "Bold", "icon": "bold-glyph"},
    {"id": "editing-toolbar:toggle-italics", "name": "Italics", "icon": "italic-glyph"},
    {"id": "SubmenuCommands-header", "name": "submenu", "icon": "header-n", "SubmenuCommands": [
      {"id": "editing-toolbar:header1-text", "name": "Header 1", "icon": "header-1"}
    ]},
    {"id": "editing-toolbar:toggle-format-brush", "name": "Format Brush", "icon": "paintbrush"},
    {"id": "editing-toolbar:change-font-color", "name": "Change font color[html]", "icon": "<svg/>"}
  ]
}
JSON
SUM=$(sha256sum "$TMP/.obsidian/plugins/editing-toolbar/data.json")
printf 'word here\nnext\n' > "$TMP/n.md"
cat > "$TMP/t.script" <<'SCRIPT'
wait 800
# the bar's right-click menu opens Settings at the toolbar's page
rclick 1200 46
wait 300
key down
key enter
wait 600
bounds tb-row:0
key escape
wait 300
# the command does too
key ctrl-p
type customise toolbar
key enter
wait 600
scroll 400
wait 300
# italic before bold; bold into the headings menu, before Heading 1
dragk tb-row:1 tb-row:0 -6
wait 300
dragk tb-row:1 tb-row:2.0 -6
wait 300
# a command from the list into the menu's end, another at the toolbar's end with +
dragk tb-cmd:toggle-highlight tb-end:1
wait 300
clickk tb-add-probe:toggle-strikethrough
wait 300
# remove the unavailable Format Brush
clickk tb-remove-probe:2
wait 300
key escape
wait 300
# colours: pick the third of your colours for "word"
clickat 0
key ctrl-home
key shift-ctrl-right
clickk tb-picker:cMenuFontColor
wait 400
clickk tb-picker:cMenuFontColor 63 79
wait 300
state
# the button now applies that colour: again removes it
clickk tb-picker:cMenuFontColor -22
wait 300
state
clickk tb-picker:cMenuFontColor -22
wait 300
state
key ctrl-s
wait 1200
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/t.script" timeout 40 target/release/tephra "$TMP/n.md" | grep -E "^(STATE|BOUNDS|DRAGK|CLICKK)")
echo "$OUT"
fail() { echo "ui-toolbar-layout: $1"; exit 1; }
grep -qE '^BOUNDS tb-row:0 [0-9]' <<<"$OUT" || fail "right-click menu didn't open the toolbar settings"
grep -qE 'DRAGK|CLICKK' <<<"$OUT" && fail "an element wasn't on screen"
SAVED="$TMP/.tephra/plugins/editing-toolbar.json"
[ -f "$SAVED" ] || fail "nothing saved in .tephra"
LAYOUT=$(python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
def ids(m): return [i["id"].replace("editing-toolbar:","") + ("[" + ",".join(c["id"].replace("editing-toolbar:","") for c in i["SubmenuCommands"]) + "]" if "SubmenuCommands" in i else "") for i in m]
print(" ".join(ids(d["menuCommands"])))
print(d["cMenuFontColor"], ",".join(d["recentFontColors"]))
print(d["menuCommands"][1].get("icon"))' "$SAVED")
echo "$LAYOUT"
WANT='toggle-italics SubmenuCommands-header[toggle-bold,header1-text,toggle-highlight] change-font-color toggle-strikethrough
#245bdb #245bdb
header-n'
[ "$LAYOUT" = "$WANT" ] || { echo "expected:"; echo "$WANT"; fail "layout or colour"; }
grep -qF "text=\"<font color='#245bdb'>word|</font> here\"" <<<"$OUT" || fail "picked colour"
grep -qF 'text="word| here"' <<<"$OUT" || fail "button toggles the colour off"
[ "$(head -1 "$TMP/n.md")" = '<font color="#245bdb">word</font> here' ] || fail "saved note: $(head -1 "$TMP/n.md")"
[ "$(sha256sum "$TMP/.obsidian/plugins/editing-toolbar/data.json")" = "$SUM" ] || fail ".obsidian was written"
echo "ui-toolbar-layout: OK"
