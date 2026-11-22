#!/usr/bin/env python3
"""Model benchmark for HANDOFF §4.2. Models only see spec.md + context files;
hidden tests stay here. Resumable: combos already in results/scores.jsonl are skipped.

  bench/run_bench.py --models qwen3.6-35b gpt-oss-20b --runs 3
  bench/run_bench.py --score-ref      # validate hidden tests against reference solutions
"""
from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(ROOT / "tools"))
from delegate import delegate  # noqa: E402

TASKS = HERE / "tasks"
WORK = HERE / "work"
RESULTS = HERE / "results"
TS_ENV = HERE / "ts-env"
CARGO_TARGET = WORK / ".cargo-target"

# task id -> (kind, expected output paths, context files relative to task dir, mode)
TASK_DEFS = {
    "t1-wikilink":   ("rust", ["src/lib.rs"], [], "files"),
    "t2-headings":   ("rust", ["src/lib.rs"], [], "files"),
    "t3-highlight":  ("ts",   ["src/highlight.ts"], [], "files"),
    "t4-mathranges": ("ts",   ["src/mathRanges.ts"], [], "files"),
    "t5-callouts":   ("ts",   ["src/callouts.ts"], [], "files"),
    "t6-css":        ("css",  ["styles/callouts.css"], [], "files"),
    "t7-bugfix":     ("ts",   ["src/lineIndex.ts"], ["src/lineIndex.ts", "lineIndex.test.ts"], "files"),
    "t8-multifile":  ("ts",   ["src/slug.ts", "src/wordcount.ts", "src/index.ts", "README.md"], [], "files"),
    "t9-longdiff":   ("diff", ["src/vaultUtils.ts"], ["src/vaultUtils.ts"], "diff"),
}

TSCONFIG = {
    "compilerOptions": {
        "strict": True, "noEmit": True, "target": "es2022", "module": "esnext",
        "moduleResolution": "bundler", "allowImportingTsExtensions": True,
        "skipLibCheck": True, "types": ["node"], "lib": ["es2022", "dom"],
    },
    "include": ["src/**/*.ts"],
}


def run(cmd: list[str], cwd: Path, timeout: int = 300, env: dict | None = None) -> tuple[int, str]:
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                           env={**os.environ, **(env or {})})
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired as e:
        return 124, f"TIMEOUT {e}"


def fresh(ws: Path) -> Path:
    if ws.exists():
        shutil.rmtree(ws)
    ws.mkdir(parents=True)
    return ws


def score_rust(task: str, files: dict[str, str], ws: Path) -> dict:
    fresh(ws)
    (ws / "Cargo.toml").write_text('[package]\nname = "bench"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\n')
    src = files.get("src/lib.rs")
    if src is None:
        rs = [v for k, v in files.items() if k.endswith(".rs")]
        src = rs[0] if len(rs) == 1 else None
    if src is None:
        return {"compiled": False, "passed": 0, "total": None, "log": "no lib.rs"}
    (ws / "src").mkdir()
    (ws / "src/lib.rs").write_text(src)
    (ws / "tests").mkdir()
    shutil.copy(TASKS / task / "hidden_test.rs", ws / "tests/hidden.rs")
    rc, out = run(["cargo", "test", "--offline", "--quiet", "--test", "hidden"], ws,
                  env={"CARGO_TARGET_DIR": str(CARGO_TARGET)})
    m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    total = len(re.findall(r"^#\[test\]", (TASKS / task / "hidden_test.rs").read_text(), re.M))
    if not m:
        return {"compiled": False, "passed": 0, "total": total, "log": out[-3000:]}
    return {"compiled": True, "passed": int(m.group(1)), "total": total,
            "unsafe": "unsafe" in src, "log": out[-1500:] if rc else ""}


def ts_workspace(task: str, files: dict[str, str], ws: Path, base: dict[str, str] | None = None) -> None:
    fresh(ws)
    (ws / "node_modules").symlink_to(TS_ENV / "node_modules")
    (ws / "package.json").write_text('{"type":"module"}')
    (ws / "tsconfig.json").write_text(json.dumps(TSCONFIG))
    for rel, body in {**(base or {}), **files}.items():
        d = ws / rel
        d.parent.mkdir(parents=True, exist_ok=True)
        d.write_text(body)
    shutil.copy(TASKS / task / "hidden.test.ts", ws / "hidden.test.ts")


def run_ts_tests(task: str, ws: Path) -> dict:
    rc_tsc, out_tsc = run([str(TS_ENV / "node_modules/.bin/tsc"), "-p", "."], ws, timeout=120)
    rc, out = run(["node", "--import", "tsx", "--test", "--test-reporter=tap", "hidden.test.ts"], ws, timeout=120)
    p = re.search(r"^# pass (\d+)", out, re.M)
    f = re.search(r"^# fail (\d+)", out, re.M)
    total = len(re.findall(r'^test\(', (TASKS / task / "hidden.test.ts").read_text(), re.M))
    passed = int(p.group(1)) if p else 0
    if f and p:
        total = max(total, passed + int(f.group(1)))
    return {"compiled": rc_tsc == 0, "typecheck_log": out_tsc[-1500:] if rc_tsc else "",
            "passed": passed, "total": total, "log": out[-2500:] if passed < total else ""}


def score_ts(task: str, files: dict[str, str], ws: Path) -> dict:
    ts_workspace(task, files, ws)
    return run_ts_tests(task, ws)


CSS_TYPES = ["note", "info", "tip", "warning", "danger", "quote", "def", "thm", "lem", "cor", "prop", "rem", "pf", "proof", "algorithm"]


def css_rules(css: str) -> list[tuple[str, str]]:
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    return [(m.group(1).strip(), m.group(2)) for m in re.finditer(r"([^{}]+)\{([^{}]*)\}", css)]


def score_css(task: str, files: dict[str, str], ws: Path) -> dict:
    css = files.get("styles/callouts.css") or next((v for k, v in files.items() if k.endswith(".css")), None)
    if css is None:
        return {"compiled": False, "passed": 0, "total": 9, "log": "no css"}
    stripped = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    balanced = stripped.count("{") == stripped.count("}")
    rules = css_rules(css)
    checks = {}
    checks["balanced"] = balanced
    trip = re.compile(r"--callout-color\s*:\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*;?")
    checks["all_types"] = all(any(f'[data-callout="{t}"]' in sel and trip.search(body) for sel, body in rules) for t in CSS_TYPES)
    base = [b for s, b in rules if re.fullmatch(r"\.callout", s)]
    bb = " ".join(base)
    checks["base"] = bool(base) and trip.search(bb) is not None and re.search(r"background(-color)?\s*:\s*rgba\(\s*var\(--callout-color\)\s*,\s*0?\.1\s*\)", bb) is not None \
        and re.search(r"border-left\s*:\s*4px\s+solid\s+rgb\(\s*var\(--callout-color\)\s*\)", bb) is not None \
        and "var(--radius-m)" in bb and re.search(r"padding\s*:[^;]*var\(--size-4-3\s*,", bb) is not None
    tb = " ".join(b for s, b in rules if re.fullmatch(r"\.callout-title", s.strip()) or s.strip().endswith(" .callout-title") and "data-callout" not in s)
    checks["title"] = all(x in tb.replace(" ", "") for x in ["color:rgb(var(--callout-color))", "font-weight:var(--font-semibold)", "display:flex", "align-items:center", "gap:"])
    cb = " ".join(b for s, b in rules if s.strip() == ".callout-content")
    checks["content"] = "var(--text-normal)" in cb
    checks["collapsed"] = any(".callout.is-collapsed .callout-content" in s and re.search(r"display\s*:\s*none", b) for s, b in rules)
    checks["fold"] = any(".callout-fold" in s and "cursor: pointer" in b.replace("cursor:pointer", "cursor: pointer") for s, b in rules) and \
        any("is-collapsed" in s and ".callout-fold" in s and "rotate(-90deg)" in b for s, b in rules)
    pf = [(s, b) for s, b in rules if '[data-callout="pf"]' in s or '[data-callout="proof"]' in s]
    checks["proof"] = any(re.search(r"background(-color)?\s*:\s*transparent", b) for s, b in pf) and any("callout-title" in s and "italic" in b for s, b in pf)
    no_trip = trip.sub("", stripped)
    named = r"\b(red|blue|green|black|white|gray|grey|orange|purple|yellow|pink|cyan|magenta|navy|teal|maroon|olive|silver|lime|aqua|fuchsia)\b"
    checks["no_literal_colors"] = not re.search(r"#[0-9a-fA-F]{3,8}\b", no_trip) and not re.search(r"rgba?\(\s*\d", no_trip) \
        and not re.search(named, re.sub(r"--[\w-]+", "", no_trip))
    passed = sum(bool(v) for v in checks.values())
    return {"compiled": balanced, "passed": passed, "total": len(checks), "checks": checks}


def score_diff(task: str, diff: str | None, ws: Path) -> dict:
    orig = (TASKS / task / "src/vaultUtils.ts").read_text()
    ts_workspace(task, {}, ws, base={"src/vaultUtils.ts": orig})
    total = len(re.findall(r'^test\(', (TASKS / task / "hidden.test.ts").read_text(), re.M)) + 1
    if not diff:
        return {"compiled": False, "applied": False, "passed": 0, "total": total, "log": "no diff"}
    (ws / "patch.diff").write_text(diff)
    rc, out = run(["patch", "-p1", "--forward", "--batch", "--fuzz=3", "-i", "patch.diff"], ws, timeout=30)
    if rc != 0:
        return {"compiled": False, "applied": False, "passed": 0, "total": total, "log": out[-1500:]}
    new = (ws / "src/vaultUtils.ts").read_text()
    fn_start = orig.index("export interface SyncConflict")
    fn_end = orig.index("\n}\n", orig.index("export function parseSyncConflict")) + 3
    lo, hi = orig[:fn_start].count("\n"), orig[:fn_end].count("\n")
    touched = [i for tag, i1, i2, _, _ in difflib.SequenceMatcher(None, orig.splitlines(), new.splitlines()).get_opcodes()
               if tag != "equal" for i in range(i1, max(i2, i1 + 1))]
    scoped = all(lo <= i <= hi for i in touched)
    r = run_ts_tests(task, ws)
    r["applied"] = True
    r["scoped"] = scoped
    r["passed"] += int(scoped)
    r["total"] = total
    return r


def score(task: str, kind: str, files: dict[str, str], diff: str | None, ws: Path) -> dict:
    if kind == "rust":
        return score_rust(task, files, ws)
    if kind == "ts":
        return score_ts(task, files, ws)
    if kind == "css":
        return score_css(task, files, ws)
    return score_diff(task, diff, ws)


def load_done() -> set[tuple[str, str, int]]:
    f = RESULTS / "scores.jsonl"
    if not f.exists():
        return set()
    return {(r["model"], r["task"], r["run"], r.get("reasoning", "default")) for r in map(json.loads, f.read_text().splitlines())}


def bench(models: list[str], tasks: list[str], runs: int, max_tokens: int, timeout: int, reasoning: str) -> None:
    RESULTS.mkdir(exist_ok=True)
    done = load_done()
    for model in models:              # grouped by model: one swap per model
        for task in tasks:
            kind, expected, ctx, mode = TASK_DEFS[task]
            for run_i in range(1, runs + 1):
                if (model, task, run_i, reasoning) in done:
                    continue
                stage = WORK / "staging" / f"{model}@{reasoning}" / task / str(run_i)
                if stage.exists():
                    shutil.rmtree(stage)
                r = delegate(model=model, task_text=(TASKS / task / "spec.md").read_text(),
                             context_files=[TASKS / task / c for c in ctx], task_id=f"bench-{task}-{model}-{run_i}",
                             mode=mode, max_tokens=max_tokens, timeout=timeout,
                             context_root=TASKS / task, staging_dir=stage,
                             extra_log={"bench": True}, reasoning=reasoning)
                s = score(task, kind, r["parsed_files"], r["diff"], WORK / "ws" / f"{model}@{reasoning}" / task / str(run_i))
                exact_paths = sorted(r["files"]) == sorted(expected) if mode == "files" else bool(r["diff"])
                rec = {"model": model, "task": task, "run": run_i, "reasoning": reasoning, "kind": kind,
                       "parsed": r["parsed"], "format_ok": bool(r["compliant"] and exact_paths),
                       "format_problems": r["problems"], "files": r["files"],
                       "prompt_tokens": r["prompt_tokens"], "completion_tokens": r["completion_tokens"],
                       "wall_s": r["wall_s"], "gen_tps": r["gen_tps"], "finish_reason": r["finish_reason"],
                       "error": r["error"], **{k: v for k, v in s.items()}}
                with (RESULTS / "scores.jsonl").open("a") as f:
                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                print(f"{model:22} {reasoning:7} {task:14} #{run_i} fmt={int(rec['format_ok'])} comp={int(bool(s.get('compiled')))} "
                      f"pass={s.get('passed')}/{s.get('total')} tok={r['completion_tokens']} {r['wall_s']}s", flush=True)


def score_ref() -> int:
    bad = 0
    for task, (kind, expected, ctx, mode) in TASK_DEFS.items():
        ref = HERE / "refs" / task
        if not ref.exists():
            print(f"{task}: no reference"); continue
        files = {str(p.relative_to(ref)): p.read_text() for p in ref.rglob("*") if p.is_file() and p.name != "patch.diff"}
        diff = (ref / "patch.diff").read_text() if (ref / "patch.diff").exists() else None
        s = score(task, kind, files, diff, WORK / "ws" / "_ref" / task)
        ok = s.get("passed") == s.get("total") and s.get("compiled")
        bad += not ok
        print(f"{task:14} {'OK ' if ok else 'BAD'} {s.get('passed')}/{s.get('total')}", "" if ok else
              json.dumps({k: v for k, v in s.items() if k in ("log", "checks", "typecheck_log")})[:3000])
    return bad


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", nargs="*", default=[])
    ap.add_argument("--tasks", nargs="*", default=list(TASK_DEFS))
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--max-tokens", type=int, default=12000)
    ap.add_argument("--timeout", type=int, default=900)
    ap.add_argument("--score-ref", action="store_true")
    ap.add_argument("--reasoning", choices=["default", "low"], default="low")
    a = ap.parse_args()
    if a.score_ref:
        return score_ref()
    bench(a.models, a.tasks, a.runs, a.max_tokens, a.timeout, a.reasoning)
    return 0


if __name__ == "__main__":
    sys.exit(main())
