#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

prod_inherited_output="$(CODETERM_TEST_KILL_DRY_RUN=1 CODETERM_API_PORT=7685 ./build.sh kill 2>&1)"
if ! grep -q "Done." <<< "$prod_inherited_output"; then
  echo "expected inherited prod port to fall back to canonical dev kill" >&2
  echo "$prod_inherited_output" >&2
  exit 1
fi
filtered_prod_output="$(
  printf '%s\n' "$prod_inherited_output" \
    | grep -v 'ignoring inherited CODETERM_API_PORT=7685' \
    | grep -v 'ignoring inherited prod CodeTerm port' \
    || true
)"
if grep -q "7685" <<< "$filtered_prod_output"; then
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
if ! grep -Eq 'refusing to (target prod CodeTerm port|run kill against prod port) 7685' <<< "$named_prod_output"; then
  echo "expected explicit prod refusal message" >&2
  echo "$named_prod_output" >&2
  exit 1
fi

default_policy="$(CODETERM_TEST_PRINT_BUILD_CACHE_POLICY=1 ./build.sh gateway-instance-policy)"
named_policy="$(CODETERM_INSTANCE=provider-model-oracle CODETERM_API_PORT=7787 CODETERM_TEST_PRINT_BUILD_CACHE_POLICY=1 ./build.sh gateway-instance-policy)"
grep -q "identifier=com.codeterm.dev$" <<< "$default_policy" || {
  echo "default instance identifier changed unexpectedly" >&2
  echo "$default_policy" >&2
  exit 1
}
grep -q "identifier=com.codeterm.dev.provider.model.oracle" <<< "$named_policy" || {
  echo "named instance must have its own WebView2 identifier" >&2
  echo "$named_policy" >&2
  exit 1
}
grep -q "api_port=7787" <<< "$named_policy" || {
  echo "named instance must preserve requested API port" >&2
  echo "$named_policy" >&2
  exit 1
}
if grep -q "CARGO_TARGET_DIR=.*codeterm-worktrees" <<< "$named_policy"; then
  echo "default policy must not inject a shared canonical target dir" >&2
  echo "$named_policy" >&2
  exit 1
fi

held_gateway_policy="$(CODETERM_INSTANCE=pw1 CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait)"
released_gateway_policy="$(CODETERM_INSTANCE=pw1 CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait --release-gateway-after-ready)"
grep -q "release_gateway_after_ready=0" <<< "$held_gateway_policy" || {
  echo "named interactive dev must retain lifetime build-gateway ownership" >&2
  exit 1
}
grep -q "release_gateway_after_ready=1" <<< "$released_gateway_policy" || {
  echo "parallel harness mode must release the build gateway after readiness" >&2
  exit 1
}
if CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait --release-gateway-after-ready >/dev/null 2>&1; then
  echo "canonical dev must reject readiness-scoped gateway ownership" >&2
  exit 1
fi

powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/check-windows-dev-cleanup-selection.ps1

echo "build kill safety check passed"
