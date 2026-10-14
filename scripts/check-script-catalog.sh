#!/usr/bin/env bash
# Caller: build.sh release-preflight/verify; CI script-catalog.yml
# Fails on a scripts/ entry with no caller header or catalog-extra entry, with no caller anywhere, or on a stale README.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
source scripts/gen-script-readme.sh

CORPUS=(build.sh build.ps1 .github .agents AGENTS.md package.json '*/package.json' scripts prod-docs docs evals .githooks)
failed=0
while IFS= read -r entry; do
  if [ -z "$(script_caller "$entry")" ]; then
    echo "ERROR: scripts/$entry has no 'Caller:' header and no scripts/catalog-extra.txt entry" >&2
    failed=1
  fi
  stem=""
  case "$entry" in *.py) stem="${entry%.py}" ;; esac
  hits=0
  for needle in "$entry" ${stem:+"$stem"}; do
    n="$({ git grep -lF -- "$needle" -- "${CORPUS[@]}" ":!scripts/$entry" ":!scripts/$entry/*"       ':!scripts/README.md' ':!scripts/catalog-extra.txt' ':!scripts/catalog-removed.md' || true; } | wc -l)"
    hits=$((hits + n))
  done
  if [ "$hits" -eq 0 ]; then
    echo "ERROR: scripts/$entry is called by nothing; delete it (record the grep evidence in scripts/catalog-removed.md) or wire it" >&2
    failed=1
  fi
done < <(script_catalog_entries)
bash scripts/gen-script-readme.sh --check || failed=1
exit "$failed"
