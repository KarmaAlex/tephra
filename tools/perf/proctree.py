#!/usr/bin/env python3
"""Launch an app, wait until it reports ready, then measure memory across its
whole process tree (main + WebKit/GPU/helper children). Standard library only.

  proctree.py [--ready-regex RE] [--settle S] [--timeout T] [--report] -- cmd args...

Prints one JSON object: cold-start ms (spawn -> ready line), and RSS / PSS / USS
sums in MiB plus a per-process breakdown. With --report it also waits for a
"REPORT {json}" line and includes it.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import select
import signal
import subprocess
import sys
import time
from pathlib import Path


def children_map() -> dict[int, list[int]]:
    m: dict[int, list[int]] = {}
    for d in Path("/proc").iterdir():
        if not d.name.isdigit():
            continue
        try:
            stat = (d / "stat").read_text()
        except OSError:
            continue
        ppid = int(stat[stat.rindex(")") + 2:].split()[1])
        m.setdefault(ppid, []).append(int(d.name))
    return m


def tree(root: int) -> list[int]:
    m = children_map()
    out, stack = [], [root]
    while stack:
        p = stack.pop()
        out.append(p)
        stack.extend(m.get(p, []))
    return out


def mem(pid: int) -> dict | None:
    try:
        roll = Path(f"/proc/{pid}/smaps_rollup").read_text()
        comm = Path(f"/proc/{pid}/comm").read_text().strip()
    except OSError:
        return None
    kb = {k: int(v) for k, v in re.findall(r"^(\w+):\s+(\d+) kB", roll, re.M)}
    uss = kb.get("Private_Clean", 0) + kb.get("Private_Dirty", 0)
    return {"pid": pid, "comm": comm, "rss": kb.get("Rss", 0) / 1024, "pss": kb.get("Pss", 0) / 1024, "uss": uss / 1024}


def measure(root_pids: list[int], exclude: set[int] = frozenset()) -> dict:
    seen, procs = set(exclude), []
    for r in root_pids:
        for p in tree(r):
            if p in seen:
                continue
            seen.add(p)
            m = mem(p)
            if m and m["rss"] > 0:
                procs.append(m)
    tot = {k: round(sum(p[k] for p in procs), 1) for k in ("rss", "pss", "uss")}
    return {**{f"{k}_mib": v for k, v in tot.items()}, "procs": sorted(
        ({**p, "rss": round(p["rss"], 1), "pss": round(p["pss"], 1), "uss": round(p["uss"], 1)} for p in procs),
        key=lambda p: -p["rss"])}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ready-regex", default=r"^READY")
    ap.add_argument("--settle", type=float, default=5.0, help="seconds to idle after ready before sampling")
    ap.add_argument("--timeout", type=float, default=60)
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--extra-root-regex", help="also include processes whose cmdline matches (for apps that re-parent helpers)")
    ap.add_argument("--exclude-root", action="store_true", help="do not count the launched wrapper itself")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd and a.cmd[0] == "--" else a.cmd
    t0 = time.monotonic()
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1,
                         start_new_session=True)
    ready_ms, report, lines = None, None, []
    rx = re.compile(a.ready_regex)
    deadline = t0 + a.timeout
    assert p.stdout
    while time.monotonic() < deadline:
        r, _, _ = select.select([p.stdout], [], [], 0.05)
        if not r:
            if p.poll() is not None:
                break
            continue
        line = p.stdout.readline()
        if not line:
            break
        lines.append(line.rstrip())
        if ready_ms is None and rx.search(line):
            ready_ms = round((time.monotonic() - t0) * 1000, 1)
            if not a.report:
                break
        if line.startswith("REPORT "):
            report = json.loads(line[7:])
            break
    if ready_ms is not None and not a.report:
        end = time.monotonic() + a.settle
        while time.monotonic() < end:
            r, _, _ = select.select([p.stdout], [], [], 0.1)
            if r:
                line = p.stdout.readline()
                if line:
                    lines.append(line.rstrip())
    roots = [p.pid]
    if a.extra_root_regex:
        rx2 = re.compile(a.extra_root_regex)
        for d in Path("/proc").iterdir():
            if d.name.isdigit():
                try:
                    if rx2.search((d / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")):
                        roots.append(int(d.name))
                except OSError:
                    pass
    m = measure(roots, {p.pid} if a.exclude_root else set()) if p.poll() is None else {}
    try:
        os.killpg(p.pid, signal.SIGTERM)
        p.wait(5)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    print(json.dumps({"cmd": cmd[0], "ready_ms": ready_ms, **m, "report": report,
                      "log_tail": [l for l in lines if not l.startswith("REPORT")][-8:]}, indent=1))
    return 0 if ready_ms is not None else 1


if __name__ == "__main__":
    sys.exit(main())
