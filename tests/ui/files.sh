#!/usr/bin/env bash
# Files that aren't notes: a .css opens as plain text (one leaf, no Markdown) titled with its
# full name; an image opens in an image tab (replacing the active tab, like a note); clicking
# a note opens the editor again.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf '# Note\n**bold**\n' > "$TMP/A.md"
printf '/* **not bold** */\n# not a heading\n.a { color: red }\n' > "$TMP/style.css"
python3 -c "
import zlib,struct,sys
def chunk(t,d): return struct.pack('>I',len(d))+t+d+struct.pack('>I',zlib.crc32(t+d)&0xffffffff)
raw=b''.join(b'\x00'+b'\xff\x00\x00'*40 for _ in range(30))
open(sys.argv[1],'wb').write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',40,30,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
" "$TMP/pic.png"
cat > "$TMP/s.script" <<SCRIPT
wait 800
proj
ws-open style.css
wait 200
proj
ws-tabs
${SHOT:+shot ${SHOT}-css.png}
ws-open pic.png
wait 300
ws-tabs
${SHOT:+shot ${SHOT}-img.png}
ws-open A.md
wait 200
ws-tabs
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/A.md" | grep -E "^(PROJ|TABS)")
echo "$OUT"
WANT='PROJ plain=false code_leaf=false blocks=3
PROJ plain=true code_leaf=true blocks=1
TABS active=0 ["style.css"]
TABS active=0 ["view:app:image"]
TABS active=0 ["A.md"]'
[ "$OUT" = "$WANT" ] || { echo "ui-files: expected:"; echo "$WANT"; exit 1; }
echo "ui-files: OK"
