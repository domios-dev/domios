#!/usr/bin/env python3
# Caller: skill crate-split; docs/architecture/split-metrics.md
"""Crate-map metrics for docs/architecture/split-metrics.md.

Usage: python3 scripts/split-metrics.py [--root PATH] [--hot-path]   # prints the metrics as JSON and a table row
Timings are measured separately through the gateway (see split-metrics.md).
"""
import datetime, glob, json, os, re, subprocess, sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
TAURI = os.path.join(ROOT, "src-tauri")
CANDIDATES = ("src-tauri/crates/codeterm-service-core/src", "src-tauri/src")

def resolve_root(argv):
    if "--root" in argv:
        return argv[argv.index("--root") + 1]
    return next((os.path.join(ROOT, c) for c in CANDIDATES if os.path.isdir(os.path.join(ROOT, c))), os.path.join(ROOT, CANDIDATES[-1]))

def run(*args):
    return subprocess.run(args, cwd=ROOT, capture_output=True, text=True).stdout.strip()

def workspace_crates():
    crates = {}
    for m in glob.glob(os.path.join(TAURI, "crates", "*", "Cargo.toml")) + [os.path.join(ROOT, "cli", "Cargo.toml"), os.path.join(TAURI, "Cargo.toml")] + glob.glob(os.path.join(ROOT, "crates", "*", "Cargo.toml")):
        text = open(m, encoding="utf-8").read()
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"', text).group(1)
        deps = set(re.findall(r'(?m)^(codeterm[-\w]*|portable-pty)\s*=\s*\{[^}]*path\s*=', text))
        crates[name] = deps - {name}
    return crates

def count_lines(paths):
    total = 0
    for p in paths:
        total += sum(1 for _ in open(p, encoding="utf-8", errors="ignore"))
    return total

def rs_files(base):
    return [os.path.join(dp, f) for dp, _, fs in os.walk(base) for f in fs if f.endswith(".rs")]

HOT_AREAS = ("service/codex_rpc", "service/pty_io", "service/orchestrator", "service/hierarchy", "service/agent_restart",
             "service/supporter_tab_state", "api/mem_routes", "api/hook_routes", "socket/server", "sentinel")

def crate_closure():
    meta = json.loads(run("cargo", "metadata", "--format-version", "1", "--offline", "--manifest-path", os.path.join(TAURI, "Cargo.toml")))
    ws = set(meta["workspace_members"])
    pkgs = {p["id"]: p for p in meta["packages"]}
    rev = {}
    for n in meta["resolve"]["nodes"]:
        for d in n["deps"]:
            if n["id"] in ws and d["pkg"] in ws:
                rev.setdefault(d["pkg"], set()).add(n["id"])
    return ws, pkgs, rev

def crate_lines(pkg):
    base = os.path.dirname(pkg["manifest_path"])
    files = []
    for dp, dirs, fs in os.walk(base):
        dirs[:] = [d for d in dirs if d != "target" and not os.path.exists(os.path.join(dp, d, "Cargo.toml"))]
        files += [os.path.join(dp, f) for f in fs if f.endswith(".rs")]
    return count_lines(files)

def hot_path(core_root):
    ws, pkgs, rev = crate_closure()
    owner = next(i for i in ws if os.path.samefile(os.path.dirname(pkgs[i]["manifest_path"]), TAURI))
    seen, stack = {owner}, [owner]
    while stack:
        for d in rev.get(stack.pop(), ()):
            if d not in seen:
                seen.add(d); stack.append(d)
    sizes = {pkgs[i]["name"]: crate_lines(pkgs[i]) for i in seen}
    big = max(sizes, key=sizes.get)
    print("| hot area | own crate | crates rebuilt (own + transitive dependents) | largest crate on hot path | lines (incl. tests) |")
    print("|---|---|---:|---|---:|")
    for a in HOT_AREAS:
        print(f"| {a} | {pkgs[owner]['name']} | {len(seen)} | {big} | {sizes[big]:,} |")

def main():
    core_root = resolve_root(sys.argv)
    if "--hot-path" in sys.argv:
        return hot_path(core_root)
    crates = workspace_crates()
    app_src = rs_files(core_root)
    shim_re = re.compile(r"^\s*pub(?:\([a-z]+\))?\s+use\s+codeterm_[\w:{} ,*]*;", re.M)
    shims = sum(len(shim_re.findall(open(p, encoding="utf-8", errors="ignore").read())) for p in app_src)
    tangle = json.loads(run(sys.executable, os.path.join(ROOT, "scripts", "module-graph.py"), "--root", core_root, "--summary"))
    members = re.search(r"members\s*=\s*\[(.*?)\]", open(os.path.join(TAURI, "Cargo.toml"), encoding="utf-8").read(), re.S).group(1)
    metrics = {
        "date": datetime.date.today().isoformat(),
        "sha": run("git", "rev-parse", "--short=9", "HEAD"),
        "crates": len(re.findall(r'"crates/', members)) + 1,
        "shims": shims,
        "cross_crate_edges": sum(len(v) for v in crates.values()),
        "tangle_units": tangle["tangle_units"],
        "tangle_loc": tangle["tangle_loc"],
        "app_lines": count_lines(app_src),
    }
    print(json.dumps(metrics))

main()
