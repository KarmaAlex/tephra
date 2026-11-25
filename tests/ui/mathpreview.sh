#!/usr/bin/env bash
# Inline math preview (LaTeX snippets plugin, Latex Suite's math preview): while the caret is
# in an inline formula, a popup shows it rendered; not outside math, not in a math block (which
# has its own preview under the source), not in reading view. Turning the plugin's "Inline
# math preview" setting off removes it without a restart; turning it on brings it back.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
printf 'x $a^2$ y\n\n$$\nb\n$$\n' > "$TMP/A.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 900
key ctrl-home
key right 4
wait 300
popup
key end
wait 300
popup
key down 3
wait 300
popup
key ctrl-home
key right 4
wait 300
popup
plugin-set latex-suite mathPreviewEnabled false
wait 300
popup
plugin-set latex-suite mathPreviewEnabled true
wait 300
popup
key ctrl-e
wait 300
popup
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/A.md" | grep -E "^POPUP")
echo "$OUT"
WANT='POPUP Some("$a^2$")
POPUP None
POPUP None
POPUP Some("$a^2$")
POPUP None
POPUP Some("$a^2$")
POPUP None'
[ "$OUT" = "$WANT" ] || { echo "ui-mathpreview: expected:"; echo "$WANT"; exit 1; }
echo "ui-mathpreview: OK"
