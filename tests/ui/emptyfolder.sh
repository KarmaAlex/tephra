#!/usr/bin/env bash
# An empty folder is a folder, not a note: clicking it does not open an editor for it,
# its context menu's "New note here" creates the note inside it (not at the vault root),
# and a note can still be dragged onto it. (Bug fixed: `TreeEntry::is_folder()` is
# children-based, so an empty folder read as a file row and click-opened as "dir:<name>".)
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
TMP="$ROOT/vault"
mkdir -p "$TMP/.obsidian" "$TMP/Empty" "$TMP/Nested/Inner"
printf '# A\n' > "$TMP/A.md"
cat > "$ROOT/t.script" <<SCRIPT
wait 800
ws-tabs
# clicking the empty folder must not open it as a note
clickk tree:dir:Empty
wait 400
ws-tabs
# the context menu's first entry, "New note here", creates the note inside the folder
rclickk tree:dir:Empty
wait 400
key down
key enter
wait 500
ws-tabs
ws-tree
# same for a nested empty folder (expand its parent first)
clickk tree:dir:Nested
wait 300
rclickk tree:dir:Nested/Inner
wait 400
key down
key enter
wait 500
ws-tabs
# dragging a note onto the empty folder still moves it there
dragk tree:A.md tree:dir:Empty 0
wait 500
ws-tabs
ws-tree
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$ROOT/t.script" timeout 40 target/release/obsidilight "$TMP/A.md" | grep -E "^(TABS|TREE|STATUS|RCLICKK|CLICKK|DRAGK)")
echo "$OUT"
fail() { echo "ui-emptyfolder: $1"; exit 1; }
grep -qE 'RCLICKK|CLICKK|DRAGK' <<<"$OUT" && fail "an element wasn't on screen"
WANT='TABS active=0 ["A.md"]
TABS active=0 ["A.md"]
TABS active=1 ["A.md", "Empty/Untitled.md"]
TREE ["A.md", "Empty/Untitled.md"]
TABS active=2 ["A.md", "Empty/Untitled.md", "Nested/Inner/Untitled.md"]
STATUS Moved "A.md" to "Empty". Updated 0 link(s) in 0 note(s).
TABS active=2 ["Empty/A.md", "Empty/Untitled.md", "Nested/Inner/Untitled.md"]
TREE ["Empty/A.md", "Empty/Untitled.md", "Nested/Inner/Untitled.md"]'
[ "$OUT" = "$WANT" ] || { echo "expected:"; echo "$WANT"; fail "output"; }
[ -f "$TMP/Empty/Untitled.md" ] && [ -f "$TMP/Nested/Inner/Untitled.md" ] || fail "notes not created inside the empty folders"
[ ! -e "$TMP/dir:Empty" ] || fail "a file named dir:Empty was created"
[ -f "$TMP/Empty/A.md" ] || fail "the drag did not move A.md into Empty"
echo "ui-emptyfolder: OK"
