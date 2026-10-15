#!/usr/bin/env bash
# Caller: build.sh script-guards
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

isolated="$(CODETERM_INSTANCE=playwright-harness CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
grep -Fxq 'mode=isolated-named-wait' <<<"$isolated"
grep -Eq '^target=.*/\.codeterm-dev-playwright-harness/target$' <<<"$isolated"
grep -Fxq 'build_gateway=required' <<<"$isolated"

canonical="$(CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
grep -Fxq 'mode=shared' <<<"$canonical"
grep -Fxq 'build_gateway=required' <<<"$canonical"

node --test scripts/tests/dev-gateway-handoff.test.mjs

echo "isolated named dev policy check passed"
