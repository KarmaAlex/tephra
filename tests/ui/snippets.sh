#!/usr/bin/env bash
# Types LaTeX snippets in the running app (keystrokes through the real input path) using the
# vault's own snippets file and Latex Suite settings, copied into a temporary vault.
set -euo pipefail
cd "$(dirname "$0")/../.."
VAULT="${VAULT:-$HOME/dev/vault-fixture}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.obsidian/plugins/obsidian-latex-suite" "$TMP/math-jax-snippets"
cp "$VAULT/.obsidian/plugins/obsidian-latex-suite/data.json" "$TMP/.obsidian/plugins/obsidian-latex-suite/"
cp "$VAULT/math-jax-snippets/snippets.js" "$TMP/math-jax-snippets/"
printf 'a ' > "$TMP/note.md"
OUT=$(OBSIDILIGHT_SCRIPT=tests/ui/snippets.script timeout 60 target/release/obsidilight "$TMP/note.md" | grep STATE)
echo "$OUT"
expect() { grep -qF -- "$1" <<<"$OUT" || { echo "ui-snippets: FAILED, expected $1"; exit 1; }; }
expect 'text="a ${}|{}$"'
expect 'text="a ${}x^{2}|{}$"'
expect 'text="a ${}x^{2}{}$|"'
expect 'text="|"'
expect 'text="\frac{\alpha}{|}"'
echo "ui-snippets: OK"
