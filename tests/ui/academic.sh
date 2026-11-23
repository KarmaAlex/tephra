#!/usr/bin/env bash
# Academic Notes in the running app: per-section numbering (callouts, a referenced equation, a
# figure), a reference shown as "lem 1.1", its hover preview, pseudocode, the contents block. Synthetic note in a temporary vault.
set -euo pipefail
cd "$(dirname "$0")/../.."
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/n.md" <<'NOTE'
# Title
> [!def] Before sections

## First
> [!lemma] Bound
> body
^lem1

> [!thm] Main
> by [[#^lem1]] we are done

## Second
> [!algorithm] Search
> \FOR{each $v$}
> \STATE visit $v$
> \ENDFOR

$$ e = mc^2 $$
^energy

> [!figure] A picture
> text

See [[#^energy]].

```academic-toc
```

end
NOTE
LINK=$(python3 -c "print(open('$TMP/n.md').read().find('[[#^lem1]]') + 4)")
printf 'wait 800\nkey ctrl-end\nwait 300\npos %s\nquit\n' "$LINK" > "$TMP/p.script"
P=$(OBSIDILIGHT_SCRIPT="$TMP/p.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep POS)
X=$(echo "$P" | awk '{print $3+4}'); Y=$(echo "$P" | awk '{print $4+10}')
printf 'wait 800\nkey ctrl-end\nwait 300\nmove %s %s\nwait 700\nnumbers\nquit\n' "$X" "$Y" > "$TMP/a.script"
OUT=$(OBSIDILIGHT_SCRIPT="$TMP/a.script" timeout 30 target/release/obsidilight "$TMP/n.md" | grep -E "NUMBERS|HOVER")
echo "$OUT"
grep -qF 'NUMBERS ["Definition 0.1", "Lemma 1.1", "Theorem 1.1", "Algorithm 2.1", "(2.1)", "Figure 2.1"]' <<<"$OUT" || { echo "ui-academic: numbering differs"; exit 1; }
grep -qF 'HOVER Some("> [!lemma] Bound")' <<<"$OUT" || { echo "ui-academic: no preview of the referenced lemma"; exit 1; }
echo "ui-academic: OK"
