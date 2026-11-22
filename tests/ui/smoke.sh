#!/usr/bin/env bash
# Runs tests/ui/smoke.script against a temporary copy of the perf note and checks that the
# edit + undo + autosave cycle leaves the file byte-identical. Needs a display.
set -euo pipefail
cd "$(dirname "$0")/../.."
VAULT="${VAULT:-$HOME/dev/vault-fixture}"
NOTE="${NOTE:-AICoNDA primo anno/Primo semestre/Distributed computing/The MST problem.md}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian"
cp "$VAULT/.obsidian/app.json" "$TMP/.obsidian/" 2>/dev/null || true
ln -s "$VAULT/Images" "$TMP/Images"
cp "$VAULT/$NOTE" "$TMP/note.md"
OBSIDILIGHT_SCRIPT=tests/ui/smoke.script timeout 60 target/release/obsidilight "$TMP/note.md" | grep STATE
cmp "$VAULT/$NOTE" "$TMP/note.md" && echo "ui-smoke: OK (byte-identical after edit + undo + autosave)"
