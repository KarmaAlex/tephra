#!/usr/bin/env bash
# Regression test: an idle window must not keep working (the watcher once re-indexed the vault
# every second, reacting to its own file reads). Opens a small temporary vault, idles, measures CPU.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP" "$TMP.out"' EXIT
mkdir -p "$TMP/.obsidian"
printf '# A\nSome text with $x^2$.\n' > "$TMP/a.md"
printf 'wait 9000\nquit\n' > "$TMP/idle.script"
# NB: the log must live outside the vault, or the watcher sees the app's own log writes
TEPHRA_TIMING=1 TEPHRA_SCRIPT="$TMP/idle.script" timeout 30 target/release/tephra "$TMP" > "$TMP.out" 2>&1 &
APP=$!
sleep 3
a=$(awk '{print $14+$15}' /proc/$APP/stat)
sleep 5
b=$(awk '{print $14+$15}' /proc/$APP/stat)
wait $APP || true
ticks=$((b - a)); hz=$(getconf CLK_TCK)
ms=$((ticks * 1000 / hz))
batches=$(grep -c "files_changed" "$TMP.out" || true)
echo "idle CPU over 5 s: ${ms} ms; re-index batches: ${batches}"
[[ $ms -le 250 ]] || { echo "idle: too much CPU while idle"; exit 1; }
[[ $batches -eq 0 ]] || { echo "idle: the watcher re-indexed with nothing changing"; exit 1; }
echo "ui-idle: OK"
