#!/usr/bin/env bash
# In the running app, on a temporary vault: search, quick switcher, command palette,
# backlinks, outgoing links, bookmarks (and bookmarks following a rename), tags -> search.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian" "$TMP/Sub"
printf '# Alpha\n## Sec\nText [[Beta]] and ![[img.png]] and [[Missing]] #course/cs\n' > "$TMP/Alpha.md"
printf 'See [[Alpha#Sec]].\nAlpha without links.\n#course\n' > "$TMP/Sub/Beta.md"
: > "$TMP/img.png"
cat > "$TMP/search.script" <<'SCRIPT'
wait 600
key ctrl-shift-f
wait 200
type without
wait 300
key ctrl-p
wait 200
type show backlinks
wait 200
key enter
wait 400
key ctrl-p
wait 200
type show outgoing
wait 200
key enter
wait 400
key ctrl-p
wait 200
type bookmark current file
wait 200
key enter
wait 400
key ctrl-o
wait 200
type beta
wait 200
key enter
wait 400
ws-tabs
key ctrl-p
wait 200
type show backlinks
wait 200
key enter
wait 400
ws-rename Alpha.md|Sub/Alpha new.md
wait 400
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/search.script" timeout 40 target/release/tephra "$TMP/Alpha.md" | grep -E "^(SEARCH|COMMAND|BACKLINKS|OUTGOING|BOOKMARK|TABS|RENAME)")
echo "$OUT"
fail() { echo "ui-search: $1"; exit 1; }
grep -qF 'SEARCH "without" files=1 lines=1' <<<"$OUT" || fail "search"
grep -qF 'COMMAND view:backlinks:backlinks' <<<"$OUT" || fail "palette did not run the view command"
grep -qF 'BACKLINKS Alpha.md linked=[("Sub/Beta.md", 1)]' <<<"$OUT" || fail "backlinks"
grep -qF 'OUTGOING Alpha.md [("Sub/Beta.md", true), ("img.png", true), ("Missing", false)]' <<<"$OUT" || fail "outgoing links"
grep -qF 'BOOKMARK Alpha.md true' <<<"$OUT" || fail "bookmark command"
grep -qF 'TABS active=0 ["Sub/Beta.md"]' <<<"$OUT" || fail "quick switcher did not open Beta"
grep -qF 'BACKLINKS Sub/Beta.md linked=[("Alpha.md", 1)]' <<<"$OUT" || fail "backlinks follow the active note"
grep -qF '"path": "Sub/Alpha new.md"' "$TMP/.tephra/bookmarks.json" || fail "bookmark did not follow the rename"
grep -qF '"Sub/Alpha new.md"' "$TMP/.tephra/workspace.json" || fail "recent files did not follow the rename"
echo "ui-search: OK"
