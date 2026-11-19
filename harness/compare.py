#!/usr/bin/env python3
"""Compares what two inbox screens say, row by row: the Rune app's `--tree` JSON and the Exact
port's `agent … --json "tree --ax"` reply. Each tree reduces to its repositories and, under each,
its rows' `#number`, status and stack parent, in reading order. Exits 1 when they differ.

    compare.py rune-tree.json exact-tree.json
"""
import json
import re
import sys

STATUSES = {"Review requested", "Needs reviewer", "Stale draft", "Changes requested", "Approved",
            "Waiting for review", "Draft", "Watching"}
REPO = re.compile(r"^[\w.-]+/[\w.-]+$")


def texts(path):
    """Every name or value in the tree, in document order."""
    data = json.load(open(path))
    out = []

    def walk(n):
        if isinstance(n, dict):
            for key in ("name", "value"):
                v = n.get(key)
                if isinstance(v, str) and v and (not out or out[-1] != v):
                    out.append(v)
            for c in n.get("children", []):
                walk(c)
        elif isinstance(n, list):
            for c in n:
                walk(c)

    if "accessibility" in data:  # Rune
        walk(data["accessibility"])
    else:  # Exact: a flat list in navigation order
        for line in data if isinstance(data, list) else [data]:
            for e in line.get("ax", {}).get("elements", []):
                walk({k: e.get(k) for k in ("name", "value")})
    return out


def rows(path):
    out, repo, row = [], None, None
    for t in texts(path):
        if REPO.match(t):
            repo = t
        elif re.fullmatch(r"#\d+", t):
            row = {"repo": repo, "pr": t, "status": None, "parent": None}
            out.append(row)
        elif row and row["status"] is None and (t in STATUSES or re.fullmatch(r"Unreviewed \d+d", t)):
            row["status"] = t
        elif row and re.fullmatch(r"↳ #\d+", t):
            row["parent"] = t[2:]
    return [(r["repo"], r["pr"], r["status"], r["parent"]) for r in out]


def main(a, b):
    left, right = rows(a), rows(b)
    ok = left == right
    width = max([len(str(r)) for r in left] + [10])
    for i in range(max(len(left), len(right))):
        l = left[i] if i < len(left) else None
        r = right[i] if i < len(right) else None
        print(f"{'  ' if l == r else '≠ '}{str(l):<{width}}  {r}")
    print(f"{'MATCH' if ok else 'DIFFER'}: {len(left)} rows vs {len(right)}")
    return 0 if ok else 1


sys.exit(main(*sys.argv[1:3]))
