#!/usr/bin/env bash
# `make perf`: measure idle memory (all processes), cold start, typing, search and indexing.
# Results -> reports/perf-<timestamp>.jsonl (gitignored); AGENTS.md holds the summarised numbers.
#
# VAULT: vault to measure on. When empty, a synthetic benchmark vault (one math-heavy note)
# is generated in the scratch dir. NOTE: note inside the vault to open (default: the
# benchmark note). OBSIDIAN=1 also measures Obsidian on the same vault (isolated profile).
set -euo pipefail
cd "$(dirname "$0")/../.."
RUNS="${RUNS:-3}"
OUT="reports/perf-$(date +%Y%m%d-%H%M%S).jsonl"
SCRATCH="${SCRATCH:-${TMPDIR:-/tmp}/tephra-perf}"
mkdir -p reports "$SCRATCH"
# scripted/benchmark runs must not replace the real last-opened vault (src/vaults.rs)
export TEPHRA_CONFIG_DIR="$SCRATCH/config"

if [[ -z "${VAULT:-}" ]]; then
  # synthetic benchmark vault: one math-heavy note (a few hundred formulas)
  VAULT="$SCRATCH/vault"
  NOTE="bench note.md"
  mkdir -p "$VAULT/.obsidian" "$VAULT/Images"
  {
    echo "# Benchmark note"
    echo
    for i in $(seq 1 200); do
      echo "Inline formula \$x_{$i} + \\frac{a_{$i}}{b_{$i}} = \\sqrt{c_{$i}}\$ and a display block:"
      echo
      echo '$$'
      echo "\\begin{pmatrix} $i & 0 \\\\ 0 & $i \\end{pmatrix} + \\sum_{k=1}^{n} k^2"
      echo '$$'
      echo
    done
    echo "![[img.png]]"
  } > "$VAULT/$NOTE"
  printf 'PNG' > "$VAULT/Images/img.png"
else
  NOTE="${NOTE:-bench note.md}"
fi
tag() { python3 -c "import json,sys; d=json.load(sys.stdin); d.pop('log_tail',None); d['label']=sys.argv[1]; print(json.dumps(d))" "$1"; }

# the app itself: idle memory, start-up, and the typing benchmark (never saves)
if [[ -x target/release/tephra ]]; then
  for i in $(seq "$RUNS"); do
    python3 tools/perf/proctree.py --settle 6 -- target/release/tephra "$VAULT/$NOTE" | tag tephra-idle >> "$OUT"
  done
  TEPHRA_BENCH=1 python3 tools/perf/proctree.py --report --timeout 90 -- target/release/tephra "$VAULT/$NOTE" | tag tephra-bench >> "$OUT"
fi
if [[ "${OBSIDIAN:-0}" == 1 ]]; then
  PROFILE="$SCRATCH/obsidian-profile"
  mkdir -p "$PROFILE"
  ASAR=$(ls "$HOME"/.config/obsidian/obsidian-*.asar 2>/dev/null | sort -V | tail -1 || true)
  [[ -n "$ASAR" && ! -e "$PROFILE/$(basename "$ASAR")" ]] && cp "$ASAR" "$PROFILE/"
  for i in $(seq "$RUNS"); do
    python3 tools/perf/proctree.py --exclude-root --settle 10 --timeout 90 -- \
      node tools/perf/obsidian_run.mjs "$PROFILE" "$VAULT" "$NOTE" --enable-plugins | tag obsidian-idle >> "$OUT"
  done
fi
python3 - "$OUT" <<'EOF'
import json, sys, statistics as st, collections
rows = [json.loads(l) for l in open(sys.argv[1])]
g = collections.defaultdict(list)
for r in rows: g[r["label"]].append(r)
print(f"{'label':22} {'n':>2} {'ready ms':>9} {'RSS MiB':>8} {'PSS MiB':>8} {'USS MiB':>8}")
for k, rs in g.items():
    med = lambda key: st.median([r[key] for r in rs if r.get(key) is not None] or [float('nan')])
    print(f"{k:22} {len(rs):>2} {med('ready_ms'):>9.0f} {med('rss_mib'):>8.1f} {med('pss_mib'):>8.1f} {med('uss_mib'):>8.1f}")
    for r in rs:
        rep = r.get("report") or {}
        if rep: print("   bench:", json.dumps({k2: rep[k2] for k2 in ('plain','inMath','scroll','firstRenderMs','plain_frame_work','math_frame_work','plain_latency','search_ms','index_ms') if k2 in rep}))
print("raw:", sys.argv[1])
EOF
