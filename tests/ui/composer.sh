#!/usr/bin/env bash
# Note composer: merge a note into another (links follow, the source goes to the trash), then
# extract a selection into a new note (a link replaces it).
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d -p "$HOME/.cache")  # under $HOME, so the system trash is on the same disk
# the merged note goes to the system trash: remove this test's entries from it afterwards
cleanup() {
  rm -rf "$TMP"
  T="${XDG_DATA_HOME:-$HOME/.local/share}/Trash"
  for i in $(grep -l "Path=$TMP" "$T"/info/*.trashinfo 2>/dev/null); do
    n=$(basename "$i" .trashinfo); rm -rf "$T/files/$n" "$i"
  done
}
trap cleanup EXIT
mkdir -p "$TMP/.obsidian" "$TMP/.tephra/plugins" "$TMP/sub"
echo '{"confirmMerge": false}' > "$TMP/.tephra/plugins/note-composer.json"
printf '# Src\nsrc body [[Src#Src]]\n' > "$TMP/sub/Src.md"
printf 'dst\n' > "$TMP/Dst.md"
printf 'see [[Src]] and [[Src#Src|it]]\n' > "$TMP/Other.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
key ctrl-p
type merge current file
key enter
wait 300
type dst
key enter
wait 600
ws-tabs
find dst
key shift-end
key ctrl-p
type extract current selection
key enter
wait 300
type Part
key enter
wait 1500
ws-tree
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/sub/Src.md" | grep -E "^(TABS|TREE|STATUS)")
echo "$OUT"
fail() { echo "ui-composer: $1"; exit 1; }
grep -qF 'TABS active=0 ["Dst.md"]' <<<"$OUT" || fail "merged-into note not opened"
[ ! -e "$TMP/sub/Src.md" ] || fail "source not removed"
[ "$(cat "$TMP/Other.md")" = 'see [[Dst]] and [[Dst#Src|it]]' ] || fail "links: $(cat "$TMP/Other.md")"
[ "$(cat "$TMP/Part.md")" = 'dst' ] || fail "extracted note: $(cat "$TMP/Part.md" 2>&1)"
[ "$(cat "$TMP/Dst.md")" = $'[[Part]]\n\n# Src\nsrc body [[Dst#Src]]' ] || fail "merged note: $(cat "$TMP/Dst.md")"
echo "ui-composer: OK"
