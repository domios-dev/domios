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

post_ready_flow="$(sed -n '/echo "Dev ready:/,/exit 0/p' build.sh)"
grep -Fq '[ -n "$CT_INSTANCE" ]' <<<"$post_ready_flow" || {
  echo "named dev --wait must make its gateway handoff decision structurally" >&2
  exit 1
}
grep -Fq 'Named dev ready; shared build gateway handoff skipped.' <<<"$post_ready_flow" || {
  echo "named dev --wait must report its structural shared-gateway skip" >&2
  exit 1
}
if grep -Fq 'CT_RELEASE_GATEWAY_AFTER_READY' <<<"$post_ready_flow"; then
  echo "named post-ready gateway behavior must not depend on caller flags" >&2
  exit 1
fi
grep -Fq 'mkdir -p "$lock"' build.sh || {
  echo "gateway handoff must create its lock directory before writing ownership" >&2
  exit 1
}
handoff_flow="$(sed -n '/^handoff_gateway_to_dev() {/,/^}/p' build.sh)"
grep -Fq 'printf '\''%s\n'\'' "$monitor_pid" > "$lock/pid"' <<<"$handoff_flow" || {
  echo "canonical dev gateway handoff must retain its pid ownership write" >&2
  exit 1
}
grep -Fq 'printf '\''%s\n'\'' "$monitor_start" > "$lock/start_token"' <<<"$handoff_flow" || {
  echo "canonical dev gateway handoff must retain its start-token ownership write" >&2
  exit 1
}
grep -Fq 'Build gateway remains owned by dev monitor $monitor_pid until launcher $dev_pid exits.' <<<"$handoff_flow" || {
  echo "canonical dev gateway handoff message changed unexpectedly" >&2
  exit 1
}

echo "isolated named dev policy check passed"
