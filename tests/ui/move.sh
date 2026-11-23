#!/usr/bin/env bash
# M8: moving notes, attachments and folders by dragging them in the file list. Links are
# updated as on rename, an open note follows its file, and impossible moves (a folder into a
# folder inside it) do nothing.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
TMP="$ROOT/vault"
mkdir -p "$TMP/.obsidian" "$TMP/A" "$TMP/B/Sub" "$TMP/Images"
printf 'See [[A/a]], ![[Images/p.png]] and [[Sub/c#Top]]\n' > "$TMP/note.md"
printf '# A\n[[note]]\n' > "$TMP/A/a.md"
printf 'b\n' > "$TMP/B/b.md"
printf '# Top\n' > "$TMP/B/Sub/c.md"
python3 -c "
import zlib,struct,sys
def chunk(t,d): return struct.pack('>I',len(d))+t+d+struct.pack('>I',zlib.crc32(t+d)&0xffffffff)
raw=b''.join(b'\x00'+b'\xff\x00\x00'*4 for _ in range(3))
open(sys.argv[1],'wb').write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',4,3,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
" "$TMP/Images/p.png"
cat > "$ROOT/t.script" <<SCRIPT
wait 800
clickk tree:dir:B
wait 300
# a note (open in the editor) into a folder
dragk tree:A/a.md tree:dir:B 0 ${SHOT:+${SHOT}-drag.png}
wait 500
ws-tabs
# an attachment to the vault root (the empty space under the list)
clickk tree:dir:Images
wait 300
dragk tree:Images/p.png tree:end
wait 500
# a folder, with the open note in it, into another folder
dragk tree:dir:B tree:dir:A
wait 500
ws-tabs
# a folder into a folder inside it: nothing happens
dragk tree:dir:A tree:A/B/b.md
wait 500
ws-tree
# links that still resolve after a move are left as they are
ws-resolve a
ws-resolve Sub/c
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$ROOT/t.script" timeout 30 target/release/obsidilight "$TMP/A/a.md" | grep -E "^(TABS|TREE|STATUS|RESOLVE|DRAGK|CLICKK)")
echo "$OUT"
fail() { echo "ui-move: $1"; exit 1; }
grep -qE 'DRAGK|CLICKK' <<<"$OUT" && fail "an element wasn't on screen"
WANT='STATUS Moved "a.md" to "B". Updated 1 link(s) in 1 note(s).
TABS active=0 ["B/a.md"]
STATUS Moved "p.png" to the vault root. Updated 1 link(s) in 1 note(s).
STATUS Moved "B" to "A". Updated 0 link(s) in 0 note(s).
TABS active=0 ["A/B/a.md"]
TREE ["A/B/Sub/c.md", "A/B/a.md", "A/B/b.md", "note.md", "p.png"]
RESOLVE a -> Some("a.md")
RESOLVE Sub/c -> Some("c.md")'
[ "$OUT" = "$WANT" ] || { echo "expected:"; echo "$WANT"; fail "moves"; }
[ -f "$TMP/A/B/Sub/c.md" ] && [ -f "$TMP/p.png" ] && [ ! -e "$TMP/B" ] || fail "files on disk"
NOTE=$(cat "$TMP/note.md")
echo "$NOTE"
[ "$NOTE" = 'See [[a]], ![[p.png]] and [[Sub/c#Top]]' ] || fail "links in note.md"
echo "ui-move: OK"
