#!/usr/bin/env bash
# Caller: build.sh script-guards
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Policy probes must never inherit a live instance, its token or a tab to notify.
while IFS='=' read -r name _; do
  case "$name" in CODETERM_*|DOMIOS_INSTANCE|DOMIOS_API_PORT|BUILD_NOTIFY_*|DEV_NOTIFY_*) unset "$name" ;; esac
done < <(env)

isolated="$(CODETERM_DEV_INSTANCE=playwright-harness CODETERM_DEV_API_PORT=7786 CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
grep -Fxq 'mode=isolated-named' <<<"$isolated"
grep -Eq '^target=.*/src-tauri/target/named-dev$' <<<"$isolated"
grep -Fxq 'build_gateway=required' <<<"$isolated"

interactive="$(CODETERM_DEV_INSTANCE=playwright-harness CODETERM_DEV_API_PORT=7786 CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev)"
[ "$interactive" = "$isolated" ]

# One DEV build per checkout: another instance name selects the same target, never a fresh one.
other="$(CODETERM_DEV_INSTANCE=other-harness CODETERM_DEV_API_PORT=7787 CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
if [ "$other" != "$isolated" ]; then
  echo "named DEV instances must share one checkout target" >&2
  exit 1
fi

if [ "$(git rev-parse --git-dir)" = "$(git rev-parse --git-common-dir)" ]; then
  canonical="$(env -u CODETERM_DEV_INSTANCE -u CODETERM_DEV_API_PORT CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY=1 ./build.sh dev --wait)"
  grep -Fxq 'mode=shared' <<<"$canonical"
  grep -Fxq 'build_gateway=required' <<<"$canonical"
fi

first="$(CODETERM_DEV_INSTANCE=tester-one CODETERM_DEV_API_PORT=7781 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait)"
second="$(CODETERM_DEV_INSTANCE=tester-two CODETERM_DEV_API_PORT=7782 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait)"
for field in data_dir api_port vite_port cdp_port; do
  first_value="$(sed -n "s/^${field}=//p" <<<"$first")"
  second_value="$(sed -n "s/^${field}=//p" <<<"$second")"
  if [ "$first_value" = "$second_value" ]; then
    echo "named Tester instances share $field=$first_value" >&2
    exit 1
  fi
done

set +e
missing_port="$(env -u CODETERM_DEV_API_PORT CODETERM_DEV_INSTANCE=tester-no-port \
  CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait 2>&1)"
missing_port_status=$?
set -e
if [ "$missing_port_status" -ne 2 ] || ! grep -q 'requires its own CODETERM_DEV_API_PORT' <<<"$missing_port"; then
  echo "named dev must refuse to share the default API port" >&2
  echo "$missing_port" >&2
  exit 1
fi

node --test scripts/tests/dev-gateway-handoff.test.mjs scripts/tests/dev-named-target.test.mjs scripts/tests/dev-instance-policy.test.mjs \
  scripts/tests/dev-shared-target.test.mjs

echo "isolated named dev policy check passed"
