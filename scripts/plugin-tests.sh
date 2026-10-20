#!/usr/bin/env bash
# Caller: package.json script; scripts affected-tests.mjs, ci-scope.mjs, run-plugin-tests.mjs; skill provider-authoring, testing
# Runs the plugin tests the catalogue runner plugin-tests discovers (all, or one plugins/<dir>) through the local tsx CLI.
# Usage: bash scripts/plugin-tests.sh [all|<plugin-dir>]
set -euo pipefail

cd "$(dirname "$0")/.."

node_bin="${CODETERM_NODE_BIN:-node}"
suite="${1:-all}"
prefix="plugins/"
if [ "$suite" != all ]; then
  if [ ! -d "plugins/$suite" ]; then
    echo "unknown plugin test suite: $suite" >&2
    exit 2
  fi
  prefix="plugins/$suite/"
fi

discovered="$("$node_bin" scripts/test-catalogue.mjs list --entry plugin-tests)"
tests=()
while IFS= read -r t; do
  case "$t" in "$prefix"*) tests+=("$t") ;; esac
done <<< "$discovered"
if [ "${#tests[@]}" -eq 0 ]; then
  echo "no plugin tests discovered under $prefix"
  exit 0
fi

fails=0
tsx_cli="node_modules/tsx/dist/cli.mjs"
for t in "${tests[@]}"; do
  echo
  echo "════ $t ════"
  if ! "$node_bin" "$tsx_cli" "$t"; then
    fails=$((fails + 1))
    echo "  ✗ FAILED: $t"
  fi
done

echo
if [ "$fails" -eq 0 ]; then
  echo "✓ all ${#tests[@]} plugin test suites green"
  exit 0
else
  echo "✗ $fails of ${#tests[@]} plugin test suite(s) failed"
  exit 1
fi
