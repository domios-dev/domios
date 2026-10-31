#!/usr/bin/env bash
# Caller: 1.18 plan-storage lane and wave QA; 1.18.1 deletes it with the converter.
# Proves only the one-time 1.18 converter reads the legacy plan-document shape.
set -euo pipefail
ROOT="${1:-$(git rev-parse --show-toplevel)}"
cd "$ROOT"

MODULE="src-tauri/crates/codeterm-plan/src/service_plan/store_convert_118/"
MOD_LINE='^src-tauri/crates/codeterm-plan/src/service_plan/mod\.rs:[0-9]+:mod store_convert_118;$'
CALL_LINE='^src-tauri/crates/codeterm-plan/src/service_plan/store_db\.rs:[0-9]+:[[:space:]]*super::store_convert_118::convert_legacy_database\(&mut connection, &path\)\?;$'

# Prints matching lines as path:line:text; no match is empty output, a search error aborts.
search() {
  local status=0
  git grep --no-index --exclude-standard -n -E "$1" -- "$2" || status=$?
  if [ "$status" -gt 1 ]; then
    echo "search for '$1' in $2 failed with status $status" >&2
    exit "$status"
  fi
}

legacy_hits="$(search 'document_json|load_plan_value|plan_document\(' src-tauri/crates/codeterm-plan)"
reference_hits="$(search 'store_convert_118' src-tauri)"
legacy="$(grep -v "^$MODULE" <<< "$legacy_hits" || true)"
references="$(grep -v "^$MODULE" <<< "$reference_hits" || true)"
mod_lines="$(grep -cE "$MOD_LINE" <<< "$references" || true)"
call_lines="$(grep -cE "$CALL_LINE" <<< "$references" || true)"
other_references="$(grep -vE "$MOD_LINE|$CALL_LINE" <<< "$references" || true)"

failed=0
if [ -n "$legacy" ]; then
  echo "legacy plan shape read outside $MODULE:"
  echo "$legacy"
  failed=1
fi
if [ -n "$other_references" ]; then
  echo "converter referenced outside its mod line and its PlanDatabase::open call:"
  echo "$other_references"
  failed=1
fi
if [ "$mod_lines" != 1 ] || [ "$call_lines" != 1 ]; then
  echo "expected exactly one mod line and one PlanDatabase::open call, found mod=$mod_lines call=$call_lines:"
  echo "$references"
  failed=1
fi
if [ "$failed" = 1 ]; then
  exit 1
fi
echo "plan storage boundary ok: legacy shape read only in $MODULE; converter referenced by its mod line and one call"
