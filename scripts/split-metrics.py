#!/usr/bin/env python3
# Caller: skill crate-split; docs/architecture/split-metrics.md
"""Crate-map metrics for docs/architecture/split-metrics.md.

Usage: python3 scripts/split-metrics.py [--row 'check=..;test=..']   # prints the metrics as JSON and a table row
Timings are measured separately through the gateway (see split-metrics.md).
"""
import datetime, glob, json, os, re, subprocess, sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
TAURI = os.path.join(ROOT, "src-tauri")

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

def main():
    crates = workspace_crates()
    app_src = rs_files(os.path.join(TAURI, "src"))
    shim_re = re.compile(r"^\s*pub(?:\([a-z]+\))?\s+use\s+codeterm_[\w:{} ,*]*;", re.M)
    shims = sum(len(shim_re.findall(open(p, encoding="utf-8", errors="ignore").read())) for p in app_src)
    tangle = json.loads(run(sys.executable, os.path.join(ROOT, "scripts", "module-graph.py"), "--summary"))
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
