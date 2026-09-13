#!/usr/bin/env bash
# Runs every plugin-side test in the repo (TS only, via the local tsx CLI).
# Aggregator stays tiny so each plugin can keep its own runner setup;
# vitest migration is a follow-up.
set -e

cd "$(dirname "$0")/.."

case "${1:-all}" in
  all)
    tests=(
      plugins/git/tests/plugin.test.ts
      plugins/provider-claude/tests/availability.test.ts
      plugins/provider-claude/tests/detect.test.ts
      plugins/provider-claude/tests/models.test.ts
      plugins/provider-claude/tests/status.test.ts
      plugins/provider-claude/tests/plugin.test.ts
      plugins/provider-claude/tests/classify-tab-state.test.ts
      plugins/provider-claude/tests/hook-ownership.test.ts
      plugins/provider-claude/tests/file-diffs.test.ts
      plugins/provider-codex/tests/plugin.test.ts
      plugins/provider-codex/tests/activity.test.ts
      plugins/provider-codex/tests/hook-events.test.ts
      plugins/provider-codex/tests/hook-ownership.test.ts
      plugins/provider-codex/tests/hook-content-verdict.test.ts
      plugins/provider-codex/tests/session-paths.test.ts
      plugins/provider-codex/tests/trust-grant.test.ts
      plugins/provider-codex/tests/provider-helper-parity.test.ts
      plugins/provider-codex/tests/classify-tab-state.test.ts
      plugins/provider-codex/tests/usage.test.ts
      plugins/provider-codex/tests/profile-sweep.test.ts
      plugins/provider-codex/tests/windows-launch.test.ts
      plugins/provider-codex/tests/file-diffs.test.ts
      plugins/provider-opencode/tests/plugin.test.ts
      plugins/provider-cursor/tests/plugin.test.ts
      plugins/chat-preprocessor/tests/plugin.test.ts
      src/lib/plugins/__tests__/registry.test.ts
      packages/chat-ui/src/codeterm/recognize.test.ts
      packages/chat-ui/src/codeterm/templates.test.ts
      packages/chat-ui/src/codeterm/planRefs.test.ts
      packages/chat-ui/src/codeterm/SentMessageBubble.test.tsx
    )
    ;;
  git)
    tests=(
      plugins/git/tests/plugin.test.ts
    )
    ;;
  provider-claude)
    tests=(
      plugins/provider-claude/tests/availability.test.ts
      plugins/provider-claude/tests/detect.test.ts
      plugins/provider-claude/tests/models.test.ts
      plugins/provider-claude/tests/status.test.ts
      plugins/provider-claude/tests/plugin.test.ts
      plugins/provider-claude/tests/classify-tab-state.test.ts
      plugins/provider-claude/tests/hook-ownership.test.ts
      plugins/provider-claude/tests/file-diffs.test.ts
    )
    ;;
  provider-codex)
    tests=(
      plugins/provider-codex/tests/plugin.test.ts
      plugins/provider-codex/tests/activity.test.ts
      plugins/provider-codex/tests/hook-events.test.ts
      plugins/provider-codex/tests/hook-ownership.test.ts
      plugins/provider-codex/tests/hook-content-verdict.test.ts
      plugins/provider-codex/tests/session-paths.test.ts
      plugins/provider-codex/tests/trust-grant.test.ts
      plugins/provider-codex/tests/provider-helper-parity.test.ts
      plugins/provider-codex/tests/classify-tab-state.test.ts
      plugins/provider-codex/tests/usage.test.ts
      plugins/provider-codex/tests/profile-sweep.test.ts
      plugins/provider-codex/tests/windows-launch.test.ts
      plugins/provider-codex/tests/file-diffs.test.ts
    )
    ;;
  chat-preprocessor)
    tests=(
      plugins/chat-preprocessor/tests/plugin.test.ts
    )
    ;;
  *)
    echo "unknown plugin test suite: $1" >&2
    exit 2
    ;;
esac

fails=0
tsx_cli="node_modules/tsx/dist/cli.mjs"
node_bin="${CODETERM_NODE_BIN:-node}"
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
  echo "✓ all plugin test suites green"
  exit 0
else
  echo "✗ $fails test suite(s) failed"
  exit 1
fi
