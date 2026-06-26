#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

prod_inherited_output="$(CODETERM_API_PORT=7685 ./build.sh kill 2>&1)"
if ! grep -q "Done." <<< "$prod_inherited_output"; then
  echo "expected inherited prod port to fall back to canonical dev kill" >&2
  echo "$prod_inherited_output" >&2
  exit 1
fi
if grep -q "7685" <<< "$prod_inherited_output"; then
  echo "canonical kill path must not mention or target prod port 7685" >&2
  echo "$prod_inherited_output" >&2
  exit 1
fi

set +e
named_prod_output="$(CODETERM_INSTANCE=prod CODETERM_API_PORT=7685 ./build.sh kill 2>&1)"
named_prod_status=$?
set -e

if [ "$named_prod_status" -eq 0 ]; then
  echo "expected named instance on prod port to be refused" >&2
  echo "$named_prod_output" >&2
  exit 1
fi
if ! grep -q "refusing to target prod CodeTerm port 7685" <<< "$named_prod_output"; then
  echo "expected explicit prod refusal message" >&2
  echo "$named_prod_output" >&2
  exit 1
fi

echo "build kill safety check passed"
