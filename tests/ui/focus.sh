#!/usr/bin/env bash
# Focus mode: "Focus mode" (the toolbar's workplace button, the focus-mode command) hides the
# ribbon, sidebars, tab bar, other panes and status bar, leaving only the note; "Fullscreen
# focus mode" (the other button, F11) does the same and also full screens the window.
# Toggling again restores everything.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf '# Note\n\ntext\n' > "$TMP/A.md"
cat > "$TMP/s.script" <<SCRIPT
wait 800
ws-tabs
ws-focus
wait 300
ws-tabs
ws-focus
wait 300
ws-focus fullscreen
wait 500
ws-tabs
ws-focus fullscreen
wait 300
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/s.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^(FOCUS|TABS)")
echo "$OUT"
WANT='TABS active=0 ["A.md"]
FOCUS true fullscreen=false
TABS active=0 ["A.md"]
FOCUS false fullscreen=false
FOCUS true fullscreen=true
TABS active=0 ["A.md"]
FOCUS false fullscreen=false'
[ "$OUT" = "$WANT" ] || { echo "ui-focus: expected:"; echo "$WANT"; exit 1; }
echo "ui-focus: OK"
