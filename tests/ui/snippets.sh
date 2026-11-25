#!/usr/bin/env bash
# Types LaTeX snippets in the running app (keystrokes through the real input path) on a
# temporary vault, using the built-in default snippet set.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
printf 'a ' > "$TMP/note.md"
OUT=$(TEPHRA_SCRIPT=tests/ui/snippets.script timeout 60 target/release/tephra "$TMP/note.md" | grep STATE)
echo "$OUT"
expect() { grep -qF -- "$1" <<<"$OUT" || { echo "ui-snippets: FAILED, expected $1"; exit 1; }; }
expect 'text="a ${}|{}$"'
expect 'text="a ${}x^{2}|{}$"'
expect 'text="a ${}x^{2}{}$|"'
expect 'text="|"'
expect 'text="\frac{\alpha}{|}"'
echo "ui-snippets: OK"
