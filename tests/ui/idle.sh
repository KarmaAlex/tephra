#!/usr/bin/env bash
# Regression test: an idle window must not keep working (the watcher once re-indexed the vault
# every second, reacting to its own file reads). Opens the vault read-only, idles, measures CPU.
set -euo pipefail
cd "$(dirname "$0")/../.."
VAULT="${VAULT:-$HOME/dev/vault-fixture}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'wait 9000\nquit\n' > "$TMP/idle.script"
OBSIDILIGHT_TIMING=1 OBSIDILIGHT_SCRIPT="$TMP/idle.script" timeout 30 target/release/obsidilight "$VAULT" > "$TMP/out.txt" 2>&1 &
APP=$!
sleep 3
a=$(awk '{print $14+$15}' /proc/$APP/stat)
sleep 5
b=$(awk '{print $14+$15}' /proc/$APP/stat)
wait $APP || true
ticks=$((b - a)); hz=$(getconf CLK_TCK)
ms=$((ticks * 1000 / hz))
batches=$(grep -c "files_changed" "$TMP/out.txt" || true)
echo "idle CPU over 5 s: ${ms} ms; re-index batches: ${batches}"
[[ $ms -le 250 ]] || { echo "idle: too much CPU while idle"; exit 1; }
[[ $batches -eq 0 ]] || { echo "idle: the watcher re-indexed with nothing changing"; exit 1; }
echo "ui-idle: OK"
