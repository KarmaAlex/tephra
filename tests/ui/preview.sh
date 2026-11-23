#!/usr/bin/env bash
# Page preview: Ctrl+hover a link in the editor previews the note; plain hover doesn't. In the
# file list, Ctrl+hover a note previews it; moving away hides it. Turning the plugin off stops
# editor previews (Academic Notes' reference previews stay a separate policy).
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf 'see [[B]] here\n' > "$TMP/A.md"
printf '# Bee\nbody of B\n' > "$TMP/B.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 900
key ctrl-end
moveat 6
wait 600
numbers
moveat 6 ctrl
wait 600
numbers
move 700 500
wait 200
numbers
move 120 115 ctrl
wait 600
move 600 600
wait 200
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/s.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^(HOVER|PREVIEW|MOVEAT)")
echo "$OUT"
WANT='HOVER None
HOVER Some("# Bee")
HOVER None
PREVIEW B.md# "# Bee"'
[ "$OUT" = "$WANT" ] || { echo "ui-preview: expected:"; echo "$WANT"; exit 1; }
echo "ui-preview: OK"
