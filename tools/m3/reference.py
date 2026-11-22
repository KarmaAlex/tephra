#!/usr/bin/env python3
"""Compacts tools/m3/observe.mjs output (reports/m3-observe-all.jsonl, with --html) into
reports/m3-reference.jsonl: per note, the callouts (type, metadata, rendered title), reference
links (target, shown text) and algorithm line markers. Both files are gitignored (note text)."""
import json, re, sys
src = sys.argv[1] if len(sys.argv) > 1 else "reports/m3-observe-all.jsonl"
dst = sys.argv[2] if len(sys.argv) > 2 else "reports/m3-reference.jsonl"
with open(dst, "w") as out:
    for line in open(src):
        d = json.loads(line)
        secs = []
        for s in d["sections"]:
            h = s.get("html", "")
            refs = [[m.group(1), m.group(2).strip()] for m in re.finditer(r'<a[^>]*an-ref[^>]*data-href="([^"]*)"[^>]*aria-label="([^"—]*)', h)]
            secs.append({"headings": s["headings"], "callouts": [{k: c[k] for k in ("type", "meta", "title")} for c in s["callouts"]], "refs": refs,
                         "an": {k: v for k, v in s["an"].items() if k.startswith("an-algorithm-line-number") or k == "an-algorithm-keyword"}})
        out.write(json.dumps({"note": d["note"], "sections": secs}, ensure_ascii=True) + "\n")
