#!/usr/bin/env bash
# PDF export in the running app: a note with a heading, math, a callout, a table and a chart
# exports through the command palette; the PDF has pages, a bookmark per heading, the title
# in the header and "1 / n" in the footer. M7: the document is drawn like the reading view
# (tasks, tables, code blocks through the reading-view helpers) with the theme's variables,
# and a note with `cssclasses: NoBorder` (note-specific snippet) has no table/code borders.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/Export test.md" <<'NOTE'
## Section one
Text with $e^{i\pi} + 1 = 0$ and **bold**.

$$
\int_0^1 x^2\, dx = \frac{1}{3}
$$

> [!note] A callout
> inside

| a | b |
|---|---|
| 1 | 2 |

- [x] done
- [ ] open

## Section two
```chart
type: bar
labels: [x, y]
series:
  - title: S
    data: [1, 2]
```
NOTE
printf 'wait 800\nkey ctrl-p\ntype export current note\nkey enter\nwait 12000\nquit\n' > "$TMP/e.script"
mkdir -p "$TMP/typ" "$TMP/.obsidian/snippets"
printf '.NoBorder {\n--table-border-width: 0px;\n}\n' > "$TMP/.obsidian/snippets/table_border.css"
OUT=$(TEPHRA_TYPST_OUT="$TMP/typ" TEPHRA_PDF_OUT="$TMP/out.pdf" TEPHRA_SCRIPT="$TMP/e.script" timeout 40 target/release/tephra "$TMP/Export test.md" | grep -E "^PDF")
echo "$OUT"
grep -q '^PDF pages=[1-9]' <<<"$OUT" || { echo "ui-pdf: no PDF"; exit 1; }
grep -q 'warnings=\[\]' <<<"$OUT" || { echo "ui-pdf: Typst warnings"; exit 1; }
OUTLINE=$(mutool show "$TMP/out.pdf" outline)
grep -q '"Section one"' <<<"$OUTLINE" && grep -q '"Section two"' <<<"$OUTLINE" || { echo "ui-pdf: bookmarks missing: $OUTLINE"; exit 1; }
TXT=$(pdftotext -layout "$TMP/out.pdf" -)
grep -q "Export test" <<<"$TXT" && grep -qE "1 ?/ ?[0-9]+" <<<"$TXT" || { echo "ui-pdf: header/footer missing"; exit 1; }
TYP="$TMP/typ/main.typ"
grep -q '^#let b_ = 1.00 \* u_ + rgb(224, 224, 224)' "$TYP" || { echo "ui-pdf: theme border missing"; exit 1; }
grep -q '#task_(0, true)' "$TYP" && grep -q '#table_((left, left,)' "$TYP" || { echo "ui-pdf: reading-view helpers not used"; exit 1; }
# a note with cssclasses: NoBorder
printf -- '---\ncssclasses: NoBorder\n---\n| a | b |\n|---|---|\n| 1 | 2 |\n' > "$TMP/Plain.md"
OUT=$(TEPHRA_TYPST_OUT="$TMP/typ" TEPHRA_PDF_OUT="$TMP/plain.pdf" TEPHRA_SCRIPT="$TMP/e.script" timeout 40 target/release/tephra "$TMP/Plain.md" | grep -E "^PDF")
grep -q 'warnings=\[\]' <<<"$OUT" || { echo "ui-pdf: NoBorder note: $OUT"; exit 1; }
grep -q '^#let b_ = none' "$TYP" || { echo "ui-pdf: NoBorder note still has borders"; exit 1; }
echo "ui-pdf: OK"
