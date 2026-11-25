#!/usr/bin/env bash
# Graph view: opens in a tab with the vault's notes and links (filters from .obsidian/graph.json),
# settles, pressing Ctrl over a node previews it, and clicking a node opens its note; the local graph follows the active note.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
echo '{"showOrphans": false}' > "$TMP/.obsidian/graph.json"
printf '[[B]] and [[Missing]]\n' > "$TMP/A.md"
printf '[[C]]\n' > "$TMP/B.md"
printf 'end\n' > "$TMP/C.md"
printf 'nobody links here\n' > "$TMP/Orphan.md"
cat > "$TMP/s.script" <<'SCRIPT'
wait 800
key ctrl-g
wait 4000
ws-tabs
graph-move C.md
wait 500
mods ctrl
wait 700
mods none
graph-click C.md
wait 400
ws-tabs
ws-right
key ctrl-p
type open local graph
key enter
wait 2500
quit
SCRIPT
OUT=$(TEPHRA_SCRIPT="$TMP/s.script" timeout 30 target/release/tephra "$TMP/A.md" | grep -E "^(GRAPH|TABS|PREVIEW)")
echo "$OUT"
fail() { echo "ui-graph: $1"; exit 1; }
grep -q '^GRAPH build global nodes=4 edges=3' <<<"$OUT" || fail "global graph: A, B, C and the unresolved link; no orphan"
grep -q '^GRAPH settled global nodes=4' <<<"$OUT" || fail "did not settle"
grep -qF 'TABS active=1 ["A.md", "view:graph:graph"]' <<<"$OUT" || fail "graph tab"
grep -qF 'PREVIEW C.md# "end"' <<<"$OUT" || fail "Ctrl pressed while hovering a node previews it"
grep -qF 'TABS active=1 ["A.md", "C.md"]' <<<"$OUT" || fail "clicking C opens it in the graph's tab"
grep -q '^GRAPH build local nodes=2 edges=1' <<<"$OUT" || fail "local graph of C: C and B"
echo "ui-graph: OK"
