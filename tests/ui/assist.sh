#!/usr/bin/env bash
# Typing assistance in the running app: auto-paired brackets, and [[ link suggestions
# (notes, headings) accepted with Enter / Tab, dismissed with Escape; ``` fences close
# themselves, Enter continues blockquotes.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/Algoritmi"
printf '# Grafi\n## Cammini minimi\n' > "$TMP/Algoritmi/Grafi.md"
printf 'x\n' > "$TMP/Algoritmi/Alberi.md"
: > "$TMP/n.md"
OUT=$(OBSIDILIGHT_SCRIPT=tests/ui/assist.script timeout 60 target/release/obsidilight "$TMP/n.md" | grep -E "STATE|SUGGEST")
echo "$OUT"
expect() { grep -qF -- "$1" <<<"$OUT" || { echo "ui-assist: FAILED, expected $1"; exit 1; }; }
expect_re() { grep -qE -- "$1" <<<"$OUT" || { echo "ui-assist: FAILED, expected /$1/"; exit 1; }; }
expect 'text="f(x|)"'
expect 'text="f(x)|"'
expect 'text="f(x) |"'
expect 'SUGGEST sel=0 ["Grafi", '
expect 'text="[[gra|]]"'
expect 'text="[[Grafi]]|"'
expect 'SUGGEST sel=0 ["Cammini minimi"]'
expect 'text="[[Grafi]] [[Grafi#Cammini minimi]]|"'
expect 'SUGGEST none'
expect_re 'line=3 .* text="```\|"$'
expect_re 'line=4 .* text="```\|"$' # the closing fence, added on typing the opener
expect_re 'line=7 .* text="> \|"$' # Enter continues a quote...
expect_re 'line=7 .* text="\|"$' # ...and ends it on an empty quote line
echo "ui-assist: OK"
