#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

isolated="$(CODETERM_INSTANCE=playwright-harness CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
grep -Fxq 'mode=isolated-named-wait' <<<"$isolated"
grep -Eq '^target=.*/\.codeterm-dev-playwright-harness/target$' <<<"$isolated"
grep -Fxq 'build_gateway=skipped' <<<"$isolated"

canonical="$(CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
grep -Fxq 'mode=shared' <<<"$canonical"
grep -Fxq 'build_gateway=required' <<<"$canonical"

grep -Fq 'Dev startup heartbeat: stage=$LAST_STAGE' build.sh
grep -Fq 'Timeout after 600s during stage: $LAST_STAGE. Last log lines:' build.sh

echo "isolated named dev policy check passed"
