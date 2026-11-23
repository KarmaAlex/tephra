#!/usr/bin/env bash
# PDF export in the running app: a note with a heading, math, a callout, a table and a chart
# exports through the command palette; the PDF has pages, a bookmark per heading, the title
# in the header and "1 / n" in the footer.
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
OUT=$(OBSIDILIGHT_PDF_OUT="$TMP/out.pdf" OBSIDILIGHT_SCRIPT="$TMP/e.script" timeout 40 target/release/obsidilight "$TMP/Export test.md" | grep -E "^PDF")
echo "$OUT"
grep -q '^PDF pages=[1-9]' <<<"$OUT" || { echo "ui-pdf: no PDF"; exit 1; }
grep -q 'warnings=\[\]' <<<"$OUT" || { echo "ui-pdf: Typst warnings"; exit 1; }
OUTLINE=$(mutool show "$TMP/out.pdf" outline)
grep -q '"Section one"' <<<"$OUTLINE" && grep -q '"Section two"' <<<"$OUTLINE" || { echo "ui-pdf: bookmarks missing: $OUTLINE"; exit 1; }
TXT=$(pdftotext -layout "$TMP/out.pdf" -)
grep -q "Export test" <<<"$TXT" && grep -qE "1 ?/ ?[0-9]+" <<<"$TXT" || { echo "ui-pdf: header/footer missing"; exit 1; }
echo "ui-pdf: OK"
