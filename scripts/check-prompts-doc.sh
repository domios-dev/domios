#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
docs = root / "docs/PROMPTS.md"
prompts_dir = root / "src-tauri/prompts"
prompts_rs = root / "src-tauri/src/prompts.rs"

doc_pairs = {}
for line in docs.read_text(encoding="utf-8").splitlines():
    m = re.match(r"\|\s*`([^`]+\.md)`\s*\|\s*`([A-Z0-9_]+)`\s*\|", line)
    if m:
        doc_pairs[m.group(1)] = m.group(2)

files = {p.name for p in prompts_dir.glob("*.md")}
const_pairs = {
    file: const
    for const, file in re.findall(
        r"pub\s+const\s+([A-Z0-9_]+)\s*:[^=]+?=\s*include_str!\(\"../prompts/([^\"]+\.md)\"\)",
        prompts_rs.read_text(encoding="utf-8"),
        re.S,
    )
}

errors = []

missing_doc = sorted(files - doc_pairs.keys())
if missing_doc:
    errors.append("Prompt files missing from docs/PROMPTS.md:")
    errors.extend(f"  {name}" for name in missing_doc)

stale_doc = sorted(doc_pairs.keys() - files)
if stale_doc:
    errors.append("docs/PROMPTS.md rows without prompt files:")
    errors.extend(f"  {name}" for name in stale_doc)

missing_const = sorted(files - const_pairs.keys())
if missing_const:
    errors.append("Prompt files missing from src-tauri/src/prompts.rs:")
    errors.extend(f"  {name}" for name in missing_const)

stale_const = sorted(const_pairs.keys() - files)
if stale_const:
    errors.append("prompts.rs consts without prompt files:")
    errors.extend(f"  {name}" for name in stale_const)

for name in sorted(files & doc_pairs.keys() & const_pairs.keys()):
    if doc_pairs[name] != const_pairs[name]:
        errors.append(
            f"Const mismatch for {name}: docs={doc_pairs[name]} prompts.rs={const_pairs[name]}"
        )

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)

print(f"Prompt docs in sync: {len(files)} files, {len(doc_pairs)} doc rows, {len(const_pairs)} consts")
PY
