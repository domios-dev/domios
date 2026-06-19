#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
# AGENTS.md is the source of truth; CLAUDE.md just @-imports it.
source = root / "AGENTS.md"
evals = root / "evals"

text = source.read_text(encoding="utf-8")
actual = {p.name for p in evals.iterdir() if p.is_dir() and not p.name.startswith("_")}
mentioned = {
    name
    for name in re.findall(r"evals/([A-Za-z0-9_.-]+)/", text)
    if not name.startswith("_")
}

experimental = set()
for line in text.splitlines():
    if "experimental" in line.lower() or "historical" in line.lower():
        experimental.update(
            name
            for name in re.findall(r"evals/([A-Za-z0-9_.-]+)/", line)
            if not name.startswith("_")
        )

missing_dirs = sorted(mentioned - actual)
undocumented = sorted(actual - mentioned - experimental)

errors = []
if missing_dirs:
    errors.append("AGENTS.md mentions eval dirs that do not exist:")
    errors.extend(f"  evals/{name}/" for name in missing_dirs)

if undocumented:
    errors.append("evals dirs missing from AGENTS.md or experimental/historical list:")
    errors.extend(f"  evals/{name}/" for name in undocumented)

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)

print(
    f"Eval list in sync: {len(actual)} eval dirs, {len(mentioned & actual)} mentioned, {len(experimental & actual)} experimental/historical"
)
PY
