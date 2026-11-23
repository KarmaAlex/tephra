#!/usr/bin/env bash
# Daily notes and templates: today's note is created from the template (format and folder from
# .obsidian/daily-notes.json), a second open reuses it; "Insert template" fills the variables.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian" "$TMP/Templates" "$TMP/Daily"
echo '{"format": "DD-MM-YY", "folder": "Daily", "template": "Templates/Day"}' > "$TMP/.obsidian/daily-notes.json"
echo '{"dateFormat": "DD-MM-YY", "folder": "Templates"}' > "$TMP/.obsidian/templates.json"
printf '# {{title}}\nCreated {{date:YYYY}}\n' > "$TMP/Templates/Day.md"
printf 'by {{title}} at {{time:HH}}' > "$TMP/Templates/Sign.md"
printf 'start\n' > "$TMP/A.md"
TODAY=$(date +%d-%m-%y)
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
key ctrl-p
type open today
key enter
wait 400
ws-tabs
state
ws-open A.md
key ctrl-end
key ctrl-p
type insert template
key enter
wait 300
type sign
key enter
wait 300
state
key ctrl-p
type open today
key enter
wait 300
ws-tabs
wait 1200
quit
SCRIPT
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/s.script" timeout 30 target/release/obsidilight "$TMP/A.md" | grep -E "^(TABS|STATE|PICKER)")
echo "$OUT"
grep -qF "TABS active=0 [\"Daily/$TODAY.md\"]" <<<"$OUT" || { echo "ui-daily: today's note not opened"; exit 1; }
[ "$(cat "$TMP/Daily/$TODAY.md")" = "# $TODAY"$'\n'"Created $(date +%Y)" ] || { echo "ui-daily: daily note text: $(cat "$TMP/Daily/$TODAY.md")"; exit 1; }
grep -qF "PICKER \"sign\" 1 [\"Sign\"]" <<<"$OUT" || { echo "ui-daily: template picker"; exit 1; }
[ "$(cat "$TMP/A.md")" = "start"$'\n'"by A at $(date +%H)" ] || { echo "ui-daily: inserted template: $(cat "$TMP/A.md")"; exit 1; }
[ "$(ls "$TMP/Daily" | wc -l)" = 1 ] || { echo "ui-daily: second open created another note"; exit 1; }
echo "ui-daily: OK"
