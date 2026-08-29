#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$ROOT" <<'PY'
from pathlib import Path
import os
import re
import sys

root = Path(sys.argv[1])
# AGENTS.md is the source of truth; CLAUDE.md just @-imports it.
# EVALS_AGENTS_FILE is a test seam; normal use still reads AGENTS.md.
source = Path(os.environ.get("EVALS_AGENTS_FILE", root / "AGENTS.md"))
evals = root / "evals"

text = source.read_text(encoding="utf-8")
actual = {p.name for p in evals.iterdir() if p.is_dir() and not p.name.startswith("_")}
mentioned = {
    name
    for name in re.findall(r"evals/([A-Za-z0-9_.-]+)/", text)
    if not name.startswith("_")
}

missing_dirs = sorted(mentioned - actual)
missing_procedures = sorted(
    name for name in actual if not (evals / name / "README.md").is_file()
)

errors = []
if missing_dirs:
    errors.append("AGENTS.md mentions eval dirs that do not exist:")
    errors.extend(f"  evals/{name}/" for name in missing_dirs)

if missing_procedures:
    errors.append("evals dirs missing README.md procedure documents:")
    errors.extend(f"  evals/{name}/" for name in missing_procedures)

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)

print(
    f"Eval procedures in sync: {len(actual)} eval dirs, {len(actual) - len(missing_procedures)} documented"
)
PY
