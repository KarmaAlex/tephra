#!/usr/bin/env bash
# The fonts the app renders with are embedded in the binary (assets/fonts/), and the theme
# families are set to them explicitly — GPUI on Linux maps `.SystemUIFont` to the hardcoded
# "IBM Plex Sans", which is missing on machines without ttf-ibm-plex, silently changing
# text weighting there. This checks the embedded families load and the theme uses them.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf '# Note\n**bold** `code`\n' > "$TMP/A.md"
cat > "$TMP/f.script" <<SCRIPT
wait 800
fonts
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/f.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^(FONTS|FONT)")
echo "$OUT"
WANT='FONTS sans=IBM Plex Sans mono=DejaVu Sans Mono
FONT IBM Plex Sans true
FONT DejaVu Sans Mono true
FONT DejaVu Sans true'
[ "$OUT" = "$WANT" ] || { echo "ui-fonts: expected:"; echo "$WANT"; exit 1; }
echo "ui-fonts: OK"
