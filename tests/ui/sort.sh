#!/usr/bin/env bash
# File list sort orders (Obsidian's "Change sort order"): folders first (A to Z, or Z to A),
# then files by name (alphanumeric: numbers by value, case and accents ignored), modified time
# or created time, each both ways. The first order comes from Obsidian's workspace.json; the
# chosen one is saved and restored on the next start; saving a note moves it under the
# modified-time order.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
TMP="$ROOT/vault"
mkdir -p "$TMP/.obsidian" "$TMP/Folder B" "$TMP/folder a"
echo '{"left":{"children":[{"type":"leaf","state":{"type":"file-explorer","state":{"sortOrder":"byCreatedTimeReverse"}}}]}}' > "$TMP/.obsidian/workspace.json"
# created in this order (birth time)...
for f in Zeta "Lecture 10" "Èlite" "lecture 2"; do echo "# $f" > "$TMP/$f.md"; sleep 0.05; done
# ...modified in another
touch -d '2026-01-04' "$TMP/Èlite.md"
touch -d '2026-01-03' "$TMP/Zeta.md"
touch -d '2026-01-02' "$TMP/lecture 2.md"
touch -d '2026-01-01' "$TMP/Lecture 10.md"
cat > "$ROOT/a.script" <<'SCRIPT'
wait 800
ws-order
ws-sort alphabetical
ws-order
ws-sort alphabeticalReverse
ws-order
ws-sort byModifiedTime
ws-order
ws-sort byModifiedTimeReverse
ws-order
ws-sort byCreatedTime
ws-order
quit
SCRIPT
cat > "$ROOT/b.script" <<'SCRIPT'
wait 800
ws-order
ws-sort byModifiedTime
ws-open Lecture 10.md
wait 300
key ctrl-end
type !
wait 1800
ws-order
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$ROOT/a.script" timeout 30 target/release/tephra "$TMP" | grep -E "^ORDER")
OUT+=$'\n'$(TEPHRA_SCRIPT="$ROOT/b.script" timeout 30 target/release/tephra "$TMP" | grep -E "^ORDER")
echo "$OUT"
WANT='ORDER byCreatedTimeReverse ["folder a/", "Folder B/", "Zeta.md", "Lecture 10.md", "Èlite.md", "lecture 2.md"]
ORDER alphabetical ["folder a/", "Folder B/", "Èlite.md", "lecture 2.md", "Lecture 10.md", "Zeta.md"]
ORDER alphabeticalReverse ["Folder B/", "folder a/", "Zeta.md", "Lecture 10.md", "lecture 2.md", "Èlite.md"]
ORDER byModifiedTime ["folder a/", "Folder B/", "Èlite.md", "Zeta.md", "lecture 2.md", "Lecture 10.md"]
ORDER byModifiedTimeReverse ["folder a/", "Folder B/", "Lecture 10.md", "lecture 2.md", "Zeta.md", "Èlite.md"]
ORDER byCreatedTime ["folder a/", "Folder B/", "lecture 2.md", "Èlite.md", "Lecture 10.md", "Zeta.md"]
ORDER byCreatedTime ["folder a/", "Folder B/", "lecture 2.md", "Èlite.md", "Lecture 10.md", "Zeta.md"]
ORDER byModifiedTime ["folder a/", "Folder B/", "Lecture 10.md", "Èlite.md", "Zeta.md", "lecture 2.md"]'
[ "$OUT" = "$WANT" ] || { echo "ui-sort: expected:"; echo "$WANT"; exit 1; }
grep -q '"fileSortOrder": "byModifiedTime"' "$TMP/.tephra/app.json" || { echo "ui-sort: order not saved"; exit 1; }
echo "ui-sort: OK"
