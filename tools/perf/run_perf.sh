#!/usr/bin/env bash
# `make perf`: measure idle memory (all processes), cold start, typing and scroll for the
# candidates, plus the Obsidian baseline (OBSIDIAN=1). Results -> reports/perf-<timestamp>.jsonl
# (gitignored); docs/perf.md holds the summarised numbers.
set -euo pipefail
cd "$(dirname "$0")/../.."
VAULT="${VAULT:-$HOME/dev/vault-fixture}"
NOTE="${NOTE:-AICoNDA primo anno/Primo semestre/Distributed computing/The MST problem.md}"
RUNS="${RUNS:-3}"
OUT="reports/perf-$(date +%Y%m%d-%H%M%S).jsonl"
SCRATCH="${SCRATCH:-${TMPDIR:-/tmp}/obsidilight-perf}"
mkdir -p reports "$SCRATCH"
MESA=/usr/share/glvnd/egl_vendor.d/50_mesa.json
tag() { python3 -c "import json,sys; d=json.load(sys.stdin); d.pop('log_tail',None); d['label']=sys.argv[1]; print(json.dumps(d))" "$1"; }

# the app itself (M1a+): idle memory, start-up, and the typing benchmark (never saves)
if [[ -x target/release/obsidilight ]]; then
  for i in $(seq "$RUNS"); do
    python3 tools/perf/proctree.py --settle 6 -- target/release/obsidilight "$VAULT/$NOTE" | tag obsidilight-idle >> "$OUT"
  done
  OBSIDILIGHT_BENCH=1 python3 tools/perf/proctree.py --report --timeout 90 -- target/release/obsidilight "$VAULT/$NOTE" | tag obsidilight-bench >> "$OUT"
fi
if [[ "${SPIKES:-0}" == 1 && -x spikes/tauri/src-tauri/target/release/spike-tauri ]]; then
  for i in $(seq "$RUNS"); do
    env VAULT="$VAULT" NOTE="$NOTE" __EGL_VENDOR_LIBRARY_FILENAMES=$MESA WEBKIT_DISABLE_DMABUF_RENDERER=1 \
      python3 tools/perf/proctree.py --settle 6 -- spikes/tauri/src-tauri/target/release/spike-tauri | tag tauri-tuned-idle >> "$OUT"
    env VAULT="$VAULT" NOTE="$NOTE" python3 tools/perf/proctree.py --settle 6 -- spikes/tauri/src-tauri/target/release/spike-tauri | tag tauri-default-idle >> "$OUT"
  done
  env VAULT="$VAULT" NOTE="$NOTE" BENCH=1 __EGL_VENDOR_LIBRARY_FILENAMES=$MESA WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    python3 tools/perf/proctree.py --report --timeout 120 -- spikes/tauri/src-tauri/target/release/spike-tauri | tag tauri-tuned-bench >> "$OUT"
fi
if [[ "${SPIKES:-0}" == 1 && -x spikes/gpui/target/release/spike-gpui ]]; then
  for i in $(seq "$RUNS"); do
    env NOTE_PATH="$VAULT/$NOTE" MATHJAX_BUNDLE=spikes/gpui/assets/mathjax-svg.js \
      python3 tools/perf/proctree.py --settle 6 -- spikes/gpui/target/release/spike-gpui | tag gpui-idle >> "$OUT"
  done
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
