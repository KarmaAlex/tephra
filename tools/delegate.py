#!/usr/bin/env python3
"""Delegate a leaf implementation task to a local model served by llama-swap.

Standard library only. Output is written to a staging directory, never into
the source tree. Every call appends one JSON line to .orchestrator/log.jsonl.

CLI:
  tools/delegate.py --model qwen3.6-35b --task spec.md --context a.rs b.rs \
      --task-id wikilink-01 [--mode files|diff] [--max-tokens 8192] \
      [--temperature 0.2] [--timeout 600]

Library:
  from delegate import delegate
  result = delegate(model=..., task_text=..., context_files=[...], task_id=...)
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORCH = ROOT / ".orchestrator"
STAGING = ORCH / "staging"
LOG = ORCH / "log.jsonl"
BASE_URL = os.environ.get("LLAMA_SWAP_URL", "http://localhost:8080")

SYSTEM_FILES = """You are a code generator inside an automated build pipeline. A program, not a human, parses your reply.

OUTPUT FORMAT (mandatory):
- Output one or more complete files. Each file goes in its own fenced code block.
- The FIRST line inside every block must be exactly `// FILE: <relative/path>` (use this marker for every language, including CSS, Markdown and JSON).
- Write each file in full. No placeholders, no "rest unchanged", no ellipses.
- Write NOTHING outside the code blocks: no explanations, no headings, no summaries.

Example of a valid reply:
```rust
// FILE: src/lib.rs
pub fn add(a: i32, b: i32) -> i32 { a + b }
```

RULES:
- Implement exactly the spec. Keep the given function signatures and type names.
- Do not add dependencies unless the spec allows them.
- No `unsafe`. Do not swallow errors.
- Do not modify or weaken tests."""

SYSTEM_DIFF = """You are a code generator inside an automated build pipeline. A program, not a human, parses your reply.

OUTPUT FORMAT (mandatory):
- Output exactly ONE fenced code block tagged `diff` containing a unified diff (as produced by `diff -u` / `git diff`) against the context files.
- Use paths `a/<path>` and `b/<path>` in the headers, with the paths shown in the context.
- Include correct `@@` hunk headers and at least 3 lines of unchanged context around each change.
- Write NOTHING outside the code block.

RULES:
- Change only what the spec requires. Keep function signatures.
- Do not add dependencies. No `unsafe`. Do not weaken tests."""

FILE_BLOCK = re.compile(
    r"^```[^\n`]*\n[ \t]*(?://|#|<!--|/\*)[ \t]*FILE:[ \t]*(?P<path>[^\n]+?)[ \t]*(?:-->|\*/)?[ \t]*\n(?P<body>.*?)^```[ \t]*$",
    re.S | re.M,
)
ANY_BLOCK = re.compile(r"^```[^\n]*\n.*?^```[ \t]*$", re.S | re.M)
DIFF_BLOCK = re.compile(r"^```(?:diff|patch)?[ \t]*\n(?P<body>.*?)^```[ \t]*$", re.S | re.M)
THINK = re.compile(r"<think>.*?</think>", re.S)


def _safe_rel(path: str) -> str | None:
    p = path.strip().strip("`'\"")
    if not p or p.startswith("/") or "\\" in p:
        return None
    parts = Path(p).parts
    if any(part in ("..", "") for part in parts):
        return None
    return p


def parse_reply(content: str, mode: str) -> dict:
    """Return {'files': {path: text}, 'diff': str|None, 'ok': bool,
    'compliant': bool, 'problems': [...]}"""
    content = THINK.sub("", content).strip()
    problems: list[str] = []
    files: dict[str, str] = {}
    diff = None
    if mode == "diff":
        m = list(DIFF_BLOCK.finditer(content))
        if m:
            diff = m[0].group("body")
            if len(m) > 1:
                problems.append(f"{len(m)} blocks, expected 1")
        elif content.startswith(("--- ", "diff --git")):
            diff = content + "\n"
            problems.append("diff not fenced")
        else:
            problems.append("no diff block")
        outside = DIFF_BLOCK.sub("", content).strip()
    else:
        for m in FILE_BLOCK.finditer(content):
            rel = _safe_rel(m.group("path"))
            if rel is None:
                problems.append(f"unsafe path {m.group('path')!r}")
                continue
            if rel in files:
                problems.append(f"duplicate file {rel}")
            files[rel] = m.group("body")
        n_blocks = len(ANY_BLOCK.findall(content))
        if n_blocks > len(files):
            problems.append(f"{n_blocks - len(files)} fenced block(s) without a valid // FILE: header")
        if not files:
            problems.append("no file blocks")
        outside = ANY_BLOCK.sub("", content).strip()
    if outside:
        problems.append(f"prose outside blocks ({len(outside)} chars)")
    ok = bool(files) or bool(diff)
    return {"files": files, "diff": diff, "ok": ok, "compliant": ok and not problems, "problems": problems}


def _post(url: str, payload: dict, timeout: float) -> dict:
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def build_user_prompt(task_text: str, context_files: list[str | Path], root: Path) -> str:
    parts = ["# TASK\n", task_text.strip(), "\n"]
    for cf in context_files:
        p = Path(cf)
        try:
            shown = p.resolve().relative_to(root.resolve())
        except ValueError:
            shown = Path(p.name)
        parts.append(f"\n# CONTEXT FILE: {shown}\n```\n{p.read_text()}\n```\n")
    return "".join(parts)


def delegate(
    *,
    model: str,
    task_text: str,
    context_files: list[str | Path] = (),
    task_id: str,
    mode: str = "files",
    max_tokens: int = 8192,
    temperature: float = 0.2,
    timeout: float = 600,
    context_root: Path = ROOT,
    staging_dir: Path | None = None,
    extra_log: dict | None = None,
    reasoning: str = "default",
) -> dict:
    system = SYSTEM_DIFF if mode == "diff" else SYSTEM_FILES
    user = build_user_prompt(task_text, list(context_files), context_root)
    payload = {
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stream": False,
    }
    if reasoning == "low":
        # llama.cpp passes these to the chat template; templates ignore keys they don't use.
        # Qwen/GLM/Gemma: enable_thinking; gpt-oss: reasoning_effort.
        payload["chat_template_kwargs"] = {"enable_thinking": False, "reasoning_effort": "low"}
    out_dir = staging_dir or (STAGING / task_id)
    out_dir.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    error = None
    resp: dict = {}
    try:
        resp = _post(f"{BASE_URL}/v1/chat/completions", payload, timeout)
    except (urllib.error.URLError, TimeoutError, OSError, json.JSONDecodeError) as e:
        error = f"{type(e).__name__}: {e}"
    wall = time.monotonic() - t0

    content = ""
    finish = None
    if resp.get("choices"):
        ch = resp["choices"][0]
        content = ch.get("message", {}).get("content") or ""
        finish = ch.get("finish_reason")
    parsed = parse_reply(content, mode) if not error else {
        "files": {}, "diff": None, "ok": False, "compliant": False, "problems": [error]}

    (out_dir / "raw_reply.md").write_text(content)
    for rel, body in parsed["files"].items():
        dest = out_dir / "files" / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(body)
    if parsed["diff"]:
        (out_dir / "patch.diff").write_text(parsed["diff"])

    usage = resp.get("usage") or {}
    timings = resp.get("timings") or {}
    record = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "task_id": task_id,
        "model": model,
        "mode": mode,
        "reasoning": reasoning,
        "prompt_tokens": usage.get("prompt_tokens"),
        "completion_tokens": usage.get("completion_tokens"),
        "wall_s": round(wall, 2),
        "gen_tps": round(timings["predicted_per_second"], 2) if "predicted_per_second" in timings else None,
        "finish_reason": finish,
        "parsed": parsed["ok"],
        "compliant": parsed["compliant"],
        "problems": parsed["problems"],
        "files": sorted(parsed["files"]),
        "staging": str(out_dir.relative_to(ROOT)) if out_dir.is_relative_to(ROOT) else str(out_dir),
        "error": error,
    }
    if extra_log:
        record.update(extra_log)
    ORCH.mkdir(exist_ok=True)
    with LOG.open("a") as f:
        f.write(json.dumps(record, ensure_ascii=False) + "\n")
    return {**record, "content": content, "parsed_files": parsed["files"], "diff": parsed["diff"]}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model", required=True)
    ap.add_argument("--task", required=True, help="task spec file (markdown)")
    ap.add_argument("--context", nargs="*", default=[], help="context files")
    ap.add_argument("--task-id", required=True)
    ap.add_argument("--mode", choices=["files", "diff"], default="files")
    ap.add_argument("--max-tokens", type=int, default=8192)
    ap.add_argument("--temperature", type=float, default=0.2)
    ap.add_argument("--timeout", type=float, default=600)
    ap.add_argument("--reasoning", choices=["default", "low"], default="low",
                    help="low = thinking off / low effort (default; see docs/model-selection.md)")
    a = ap.parse_args()
    r = delegate(
        model=a.model, task_text=Path(a.task).read_text(), context_files=a.context,
        task_id=a.task_id, mode=a.mode, max_tokens=a.max_tokens,
        temperature=a.temperature, timeout=a.timeout, reasoning=a.reasoning,
    )
    summary = {k: r[k] for k in ("task_id", "model", "parsed", "compliant", "problems",
                                 "files", "completion_tokens", "wall_s", "staging")}
    print(json.dumps(summary, indent=2, ensure_ascii=False))
    return 0 if r["parsed"] else 1


if __name__ == "__main__":
    sys.exit(main())
