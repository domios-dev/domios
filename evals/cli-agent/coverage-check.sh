#!/usr/bin/env bash
# Report CLI commands (from docs/cli-manifest.json) with no benchmark case.
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
python3 - "$REPO_ROOT/docs/cli-manifest.json" "$HARNESS_DIR/cases" <<'PY'
import sys, json, glob, os, yaml
manifest = json.load(open(sys.argv[1]))
def paths(node, prefix=()):
    for c in node.get("subcommands", []):
        p = prefix + (c["name"],)
        if c.get("subcommands"):
            yield from paths(c, p)
        else:
            yield " ".join(p)
all_cmds = set(paths(manifest))
covered = set()
for f in glob.glob(os.path.join(sys.argv[2], "*.yaml")):
    for case in (yaml.safe_load(open(f)) or []):
        covered.add(case.get("command", ""))
missing = sorted(all_cmds - covered)
print(f"{len(covered & all_cmds)}/{len(all_cmds)} leaf commands covered")
if missing:
    print("UNCOVERED:")
    for m in missing:
        print(f"  {m}")
PY
