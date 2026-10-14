#!/usr/bin/env python3
# Caller: docs architecture module-graph.md and split-roadmap.md (manual generation)
"""Module dependency graph of the src-tauri crate (crate:: references, depth 1 and 2 for service/agent/api).

Usage: python3 scripts/module-graph.py [--json out.json] [--md out.md]
"""
import collections, json, os, re, sys
from collections import defaultdict

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "src")
DEEP = {"service", "agent", "api"}

def unit_of(rel):
    parts = rel.replace("\\", "/").split("/")
    top = parts[0][:-3] if parts[0].endswith(".rs") else parts[0]
    if top in DEEP and len(parts) > 1:
        sub = parts[1][:-3] if parts[1].endswith(".rs") else parts[1]
        return f"{top}/{sub}" if sub != "mod" else top
    return top

def is_test_file(rel):
    b = os.path.basename(rel)
    return b.endswith("_tests.rs") or b == "tests.rs" or "/tests/" in rel.replace("\\", "/") or b.startswith("tests_")

def expand(tree):
    """Expand a use-tree body after `crate::` into leaf paths."""
    tree = tree.strip()
    out = []
    def rec(prefix, t):
        t = t.strip()
        if not t: return
        i = t.find("{")
        if i == -1:
            out.append(prefix + t.split(" as ")[0].strip()); return
        head = t[:i]; body = t[i+1:t.rfind("}")]
        depth = 0; cur = ""
        for ch in body:
            if ch == "{": depth += 1
            if ch == "}": depth -= 1
            if ch == "," and depth == 0:
                rec(prefix + head, cur); cur = ""
            else: cur += ch
        rec(prefix + head, cur)
    rec("", tree)
    return out

USE = re.compile(r"\buse\s+crate::([^;]+);", re.S)
PATH = re.compile(r"\bcrate::([A-Za-z_]\w*)(?:::([A-Za-z_]\w*))?")

def unit_for_path(path):
    seg = [s for s in re.split(r"::", path) if s and s != "self"]
    if not seg: return None
    if seg[0] in DEEP and len(seg) > 1 and re.match(r"^[a-z_0-9]+$", seg[1]):
        return f"{seg[0]}/{seg[1]}"
    return seg[0]

def scan(include_tests):
    edges = defaultdict(lambda: defaultdict(int))
    loc = defaultdict(int)
    for dp, _, fs in os.walk(ROOT):
        for f in fs:
            if not f.endswith(".rs"): continue
            p = os.path.join(dp, f); rel = os.path.relpath(p, ROOT)
            if rel.replace("\\", "/") == "lib.rs": continue
            src = open(p, encoding="utf-8", errors="ignore").read()
            u = unit_of(rel)
            test = is_test_file(rel)
            if not test: loc[u] += src.count("\n")
            if test and not include_tests: continue
            if not include_tests:
                m = re.search(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+\s*\{", src)
                if m: src = src[:m.start()]
            targets = []
            for m in USE.finditer(src):
                for leaf in expand(m.group(1)):
                    targets.append(unit_for_path(leaf))
            stripped = USE.sub("", src)
            for m in PATH.finditer(stripped):
                targets.append(unit_for_path(m.group(0)[len("crate::"):]))
            for t in targets:
                if t and t != u and not (t.split("/")[0] == u.split("/")[0] and "/" not in t and "/" not in u and t == u):
                    edges[u][t] += 1
    return edges, loc

def normalize(edges):
    """Edge to bare `service` (facade items) is kept; resolve edge validity to known units later."""
    return edges

GENERIC = {
    "new", "get", "set", "len", "is_empty", "from", "default", "clone", "into", "id", "name", "list", "insert",
    "remove", "push", "take", "run", "start", "stop", "close", "open", "read", "write", "build", "update", "send",
    "next", "iter", "map", "drop", "fmt", "eq", "cmp", "hash", "parse", "load", "save", "apply", "emit", "state",
    "value", "kind", "key", "text", "path", "label", "status", "clear", "reset", "check", "init", "call", "done",
    "contains", "find", "lock", "wait", "notify", "record", "handle", "resolve", "validate", "create", "delete",
}
DEF_RE = re.compile(r"^(?P<ind>[ 	]*)(?:pub(?:\([a-z]+\))?\s+)(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+(?P<name>[A-Za-z_]\w*)", re.M)
CALL_RE = re.compile(r"(?:\.|::)([A-Za-z_]\w*)\s*(?:::<[^>]*>)?\(")

def source_files():
    for dp, _, fs in os.walk(ROOT):
        for f in fs:
            if not f.endswith(".rs"): continue
            p = os.path.join(dp, f); rel = os.path.relpath(p, ROOT).replace("\\", "/")
            if rel == "lib.rs" or is_test_file(rel): continue
            src = open(p, encoding="utf-8", errors="ignore").read()
            m = re.search(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+\s*\{", src)
            if m: src = src[:m.start()]
            yield unit_of(rel), src

def method_layer(loc, adj_in):
    defs = collections.defaultdict(set)          # name -> {unit}
    own = collections.defaultdict(set)           # unit -> {name}
    bodies = []
    for unit, src in source_files():
        bodies.append((unit, src))
        for m in DEF_RE.finditer(src):
            name = m.group("name")
            defs[name].add(unit); own[unit].add(name)
    usable = {n for n, us in defs.items() if len(us) == 1 and n not in GENERIC and len(n) >= 5}
    calls = collections.defaultdict(lambda: collections.defaultdict(int))   # name -> caller unit -> count
    for unit, src in bodies:
        for m in CALL_RE.finditer(src):
            n = m.group(1)
            if n in usable and next(iter(defs[n])) != unit and "test" not in unit:
                calls[n][unit] += 1
    rows = []
    for unit in sorted(own):
        if loc.get(unit, 0) < 400: continue
        mine = sorted(n for n in own[unit] if n in usable)
        external = [n for n in mine if n in calls]
        sites = sum(sum(calls[n].values()) for n in external)
        callers = collections.Counter()
        for n in external:
            for cu, c in calls[n].items(): callers[cu] += c
        rows.append((len(callers), sites, unit, len(mine), len(external), callers))
    rows.sort(key=lambda r: (-r[0], -r[1]))
    out = ["## Method-edge layer", "",
           "`crate::` paths miss `impl ServiceContext` methods called as `ctx.method()`. This layer scans every non-test file for calls to `pub` fns whose name is defined in exactly one unit, is at least 5 characters and not in a generic-name stoplist (so a method-name collision cannot create a false edge). Counts are call sites outside the defining unit. Ambiguous names are skipped (undercount), but a call to a same-named method of an external crate type is counted (overcount), so read hub rows as indicative, not exact.", "",
           "| unit | LOC | pub fns defined (usable) | called from outside | external call sites | caller units | path fan-in | top callers |",
           "|---|---:|---:|---:|---:|---:|---:|---|"]
    for callers_n, sites, unit, ndef, next_, callers in rows:
        if next_ == 0: continue
        top = ", ".join(f"{u} ({c})" for u, c in callers.most_common(3))
        out.append(f"| {unit} | {loc[unit]} | {ndef} | {next_} | {sites} | {callers_n} | {len(adj_in.get(unit, ()))} | {top} |")
    skipped = sum(1 for n, us in defs.items() if len(us) > 1 or n in GENERIC or len(n) < 5)
    out += ["", f"Names skipped as ambiguous or generic: {skipped} of {len(defs)} distinct pub fn names.", ""]
    return out

def sccs(nodes, adj):
    idx = {}; low = {}; st = []; on = set(); res = []; c = [0]
    sys.setrecursionlimit(10000)
    def sc(v):
        idx[v] = low[v] = c[0]; c[0] += 1; st.append(v); on.add(v)
        for w in adj.get(v, ()):
            if w not in idx: sc(w); low[v] = min(low[v], low[w])
            elif w in on: low[v] = min(low[v], idx[w])
        if low[v] == idx[v]:
            comp = []
            while True:
                w = st.pop(); on.discard(w); comp.append(w)
                if w == v: break
            res.append(comp)
    for v in nodes:
        if v not in idx: sc(v)
    return res

def main():
    edges, loc = scan(False)
    known = set(loc)
    nodes = sorted(known)
    adj = {u: {t for t in edges[u] if t in known} for u in nodes}
    inb = defaultdict(set)
    for u, ts in adj.items():
        for t in ts: inb[t].add(u)
    comps = [c for c in sccs(nodes, adj) if len(c) > 1]
    top = lambda x: x.split("/")[0]
    # top-level graph
    tadj = defaultdict(set)
    for u, ts in adj.items():
        for t in ts:
            if top(u) != top(t): tadj[top(u)].add(top(t))
    tnodes = sorted({top(n) for n in nodes})
    tcomps = [c for c in sccs(tnodes, tadj) if len(c) > 1]
    out = {"loc": dict(loc), "edges": {u: sorted(adj[u]) for u in nodes},
           "inbound": {u: sorted(inb[u]) for u in nodes}, "scc_units": comps, "scc_top": tcomps}
    if "--json" in sys.argv:
        json.dump(out, open(sys.argv[sys.argv.index("--json")+1], "w"), indent=1)
    md = ["# Module dependency graph (generated)", "",
          "Generated by `python3 scripts/module-graph.py --md docs/architecture/module-graph.md`. Edges are `crate::` references in non-test code of the `codeterm` (src-tauri) crate; `#[cfg(test)]` modules and `*_tests.rs`/`tests/` files are excluded from edges and LOC.", "",
          "## Top-level modules", "", "| module | LOC | depends on | depended on by |", "|---|---:|---|---|"]
    tin = defaultdict(set)
    for a, bs in tadj.items():
        for b in bs: tin[b].add(a)
    tl = defaultdict(int)
    for n in nodes: tl[top(n)] += loc[n]
    for t in sorted(tl, key=lambda x: -tl[x]):
        if tl[t] >= 300:
            md.append(f"| {t} | {tl[t]} | {', '.join(sorted(tadj[t]))} | {', '.join(sorted(tin[t]))} |")
    md += ["", "## Units (top-level, plus `service/*`, `agent/*`, `api/*` children) of 400+ LOC", "",
           "| unit | LOC | fan-in | fan-out | fan-in units |", "|---|---:|---:|---:|---|"]
    for n in sorted(nodes, key=lambda n: (len(inb[n]), -loc[n])):
        if loc[n] >= 400:
            ins = sorted(inb[n]); txt = ", ".join(ins) if len(ins) <= 4 else f"{len(ins)} units"
            md.append(f"| {n} | {loc[n]} | {len(inb[n])} | {len(adj[n])} | {txt} |")
    md += ["", "## Cycles (strongly connected components)", "",
           f"Top-level modules: {len(tcomps)} component(s), sizes {[len(c) for c in tcomps]}.", ""]
    for c in tcomps: md.append("- " + ", ".join(sorted(c)))
    md += ["", f"Units: {len(comps)} component(s), sizes {[len(c) for c in comps]}, LOC {[sum(loc[x] for x in c) for c in comps]}.", ""]
    for c in comps: md.append("- " + ", ".join(sorted(c)))
    md += method_layer(loc, inb)
    if "--summary" in sys.argv:
        big = max(comps, key=len) if comps else []
        print(json.dumps({"tangle_units": len(big), "tangle_loc": sum(loc[x] for x in big),
                          "top_level_in_cycle": max((len(c) for c in tcomps), default=0),
                          "app_loc_non_test": sum(loc.values())}))
        return
    if "--md" in sys.argv:
        open(sys.argv[sys.argv.index("--md")+1], "w", encoding="utf-8").write(chr(10).join(md) + chr(10))
    else:
        print(chr(10).join(md))
main()
