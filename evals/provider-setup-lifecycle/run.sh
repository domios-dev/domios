#!/usr/bin/env bash
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

if [ "${1:-}" != "--check" ]; then
  echo "usage: $0 --check" >&2
  exit 2
fi

FAILURES=0
row() {
  local label="$1"
  shift
  if "$@"; then
    printf 'PASS %s\n' "$label"
  else
    printf 'FAIL %s\n' "$label" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

configure_checks() {
  node "$ROOT/scripts/run-ts-unit-tests.mjs" pluginConfigure providerInstall &&
    "$ROOT/build.sh" test-one app configure_spawn
}

health="$(dev_get /api/health)" || exit 7
expected_sha="$(git -C "$ROOT" rev-parse HEAD)"
actual_sha="$(printf '%s' "$health" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("build_sha", ""))')"
if [ "$actual_sha" != "$expected_sha" ]; then
  printf 'FAIL exact SHA: expected=%s actual=%s\n' "$expected_sha" "$actual_sha" >&2
  exit 1
fi
dc identify --json >/dev/null || exit 7

export PROVIDER_SETUP_TASK_TOKEN="${PROVIDER_SETUP_TASK_TOKEN:-provider-setup-$RANDOM-$RANDOM}"
row "hook contract/refusal and foreign non-mutation" \
  bash "$ROOT/scripts/plugin-tests.sh" provider-codex
row "Configure exact-token delivery/failure" \
  configure_checks
row "Install Provider launch/failure" \
  "$ROOT/build.sh" test-one app install_provider_tests

exit "$FAILURES"
