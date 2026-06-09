#!/usr/bin/env bash
# Runs every plugin-side test in the repo (TS only, via npx tsx).
# Aggregator stays tiny so each plugin can keep its own runner setup;
# vitest migration is a follow-up.
set -e

cd "$(dirname "$0")/.."

case "${1:-all}" in
  all)
    tests=(
      plugins/provider-claude/tests/parser.test.ts
      plugins/provider-claude/tests/detect.test.ts
      plugins/provider-claude/tests/status.test.ts
      plugins/provider-claude/tests/plugin.test.ts
      plugins/provider-codex/tests/plugin.test.ts
      plugins/provider-opencode/tests/plugin.test.ts
      src/lib/plugins/__tests__/registry.test.ts
      packages/chat-ui/src/codeterm/recognize.test.ts
    )
    ;;
  provider-claude)
    tests=(
      plugins/provider-claude/tests/parser.test.ts
      plugins/provider-claude/tests/detect.test.ts
      plugins/provider-claude/tests/status.test.ts
      plugins/provider-claude/tests/plugin.test.ts
    )
    ;;
  *)
    echo "unknown plugin test suite: $1" >&2
    exit 2
    ;;
esac

fails=0
for t in "${tests[@]}"; do
  echo
  echo "════ $t ════"
  if ! npx tsx "$t"; then
    fails=$((fails + 1))
    echo "  ✗ FAILED: $t"
  fi
done

echo
if [ "$fails" -eq 0 ]; then
  echo "✓ all plugin test suites green"
  exit 0
else
  echo "✗ $fails test suite(s) failed"
  exit 1
fi
