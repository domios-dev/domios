#!/usr/bin/env bash
# Caller: CI docs-source-of-truth.yml; skill build, prompts
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON_BIN="$(command -v python3 || command -v python)"

"$PYTHON_BIN" - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
docs = root / "src-tauri/prompts/00-README.md"
prompts_dir = root / "src-tauri/prompts"
prompts_rs = root / "src-tauri/crates/domios-prompts/src/prompts.rs"
service_prompts_rs = root / "src-tauri/crates/domios-service-core/src/prompts.rs"
plan_prompts_rs = root / "src-tauri/crates/domios-plan/src/service_plan/prompts_plan.rs"
mem_prompts_rs = root / "src-tauri/crates/domios-mem/src/prompt_text.rs"
seed_rs = root / "src-tauri/crates/domios-mem/src/seed.rs"
plugin_prompts_rs = root / "src-tauri/crates/domios-plugin/src/prompts.rs"

doc_pairs = {}
for line in docs.read_text(encoding="utf-8").splitlines():
    m = re.match(r"\|\s*`([^`]+\.md)`\s*\|\s*`([A-Z0-9_]+)`\s*\|", line)
    if m:
        doc_pairs[m.group(1)] = m.group(2)

documentation_files = {docs}
prompt_files = [p for p in prompts_dir.rglob("*.md") if p not in documentation_files]
files = {p.relative_to(prompts_dir).as_posix() for p in prompt_files}
loader_sources = [
    prompts_rs,
    service_prompts_rs,
    plan_prompts_rs,
    mem_prompts_rs,
    seed_rs,
    plugin_prompts_rs,
]
loaders = {}
pattern = re.compile(
    r'(pub\s+)?const\s+([A-Z0-9_]+)\s*:[^=]+?=\s*include_str!\("(?:\.\./)+prompts/([^\"]+\.md)"\)',
    re.S,
)
concat_pattern = re.compile(
    r'(pub\s+)?const\s+([A-Z0-9_]+)\s*:[^=]+?=\s*concat!\(\s*include_str!\("(?:\.\./)+prompts/([^\"]+\.md)"\)',
    re.S,
)
for source in loader_sources:
    for visibility, const, file in pattern.findall(source.read_text(encoding="utf-8")) + concat_pattern.findall(source.read_text(encoding="utf-8")):
        loaders.setdefault(file, []).append((source.relative_to(root).as_posix(), const, bool(visibility)))

errors = []

bold_prompt_files = sorted(
    p.relative_to(prompts_dir).as_posix()
    for p in prompt_files
    if "**" in p.read_text(encoding="utf-8")
)
if bold_prompt_files:
    errors.append("Compiled prompts use Markdown bold instead of plain text or uppercase:")
    errors.extend(f"  {name}" for name in bold_prompt_files)

missing_doc = sorted(files - doc_pairs.keys())
if missing_doc:
    errors.append("Prompt files missing from src-tauri/prompts/00-README.md:")
    errors.extend(f"  {name}" for name in missing_doc)

stale_doc = sorted(doc_pairs.keys() - files)
if stale_doc:
    errors.append("src-tauri/prompts/00-README.md rows without prompt files:")
    errors.extend(f"  {name}" for name in stale_doc)

missing_const = sorted(files - loaders.keys())
if missing_const:
    errors.append("Prompt files missing from authoritative loader modules:")
    errors.extend(f"  {name}" for name in missing_const)

stale_const = sorted(loaders.keys() - files)
if stale_const:
    errors.append("Authoritative loader consts without prompt files:")
    errors.extend(f"  {name}" for name in stale_const)

for name in sorted(files & doc_pairs.keys() & loaders.keys()):
    loader_consts = {const for _, const, _ in loaders[name]}
    if loader_consts != {doc_pairs[name]}:
        errors.append(
            f"Const mismatch for {name}: docs={doc_pairs[name]} loaders={sorted(loader_consts)}"
        )

rule_prompt_owners = {source for source, _, _ in loaders.get("codeterm_rule.md", [])}  # rename-bridge
expected_rule_owners = {
    "src-tauri/crates/domios-prompts/src/prompts.rs",
    "src-tauri/crates/domios-plugin/src/prompts.rs",
}
if rule_prompt_owners != expected_rule_owners:
    errors.append(f"codeterm_rule.md split ownership drifted: {sorted(rule_prompt_owners)}")  # rename-bridge

seed_owner = "src-tauri/crates/domios-mem/src/seed.rs"
for name in sorted(file for file in files if file.startswith("seed/")):
    entries = loaders.get(name, [])
    if len(entries) != 1 or entries[0][0] != seed_owner or entries[0][2]:
        errors.append(f"Seed loader must be one private const in {seed_owner}: {name}")

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)

loader_count = sum(len(entries) for entries in loaders.values())
print(f"Prompt docs in sync: {len(files)} files, {len(doc_pairs)} doc rows, {loader_count} loaders")
PY
