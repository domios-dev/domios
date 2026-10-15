#!/usr/bin/env bash
# Caller: skill build
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/cache-policy.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export CT_BUILD_GATEWAY_SCOPE=fixture CT_BUILD_GATEWAY_FIXTURE_DIR="$WORK/gateway"

codeterm() {
  [ "$1 $2 $3" = 'mem secret get' ] || return 2
  case "$5" in
    domios-sccache-s3-access-key) [ "${FIXTURE_ACCESS:-1}" = 1 ] && printf fixture-access ;;
    domios-sccache-s3-secret-key) [ "${FIXTURE_SECRET:-1}" = 1 ] && printf fixture-secret ;;
    *) return 2 ;;
  esac
}
sccache() {
  case "$1" in
    --stop-server) printf 'stop\n' >> "$WORK/events"; rm -f "$WORK/location" ;;
    --start-server)
      printf 'start\n' >> "$WORK/events"
      [ "${FIXTURE_START_FAILURE:-0}" = 0 ] || return 1
      if [ "$CT_SCCACHE_STORAGE" = shared ]; then
        [ "$AWS_ACCESS_KEY_ID" = fixture-access ]
        [ "$AWS_SECRET_ACCESS_KEY" = fixture-secret ]
        [ -z "${AWS_SESSION_TOKEN:-}" ]
        [ "$SCCACHE_ENDPOINT" = http://100.64.10.5:9000 ]
        [ "$SCCACHE_BUCKET:$SCCACHE_REGION:$SCCACHE_S3_USE_SSL" = sccache:us-east-1:false ]
        printf 's3, name: sccache, prefix: /%s' "${SCCACHE_S3_KEY_PREFIX:-}" > "$WORK/location"
      else
        [ -z "${SCCACHE_BUCKET:-}${SCCACHE_ENDPOINT:-}" ]
        printf 'Local disk: "fixture-disk"' > "$WORK/location"
      fi
      ;;
    --version) printf 'sccache fixture\n' ;;
    --show-stats)
      printf 'Cache location '
      if [ -e "$WORK/location" ]; then cat "$WORK/location"; else printf fixture; fi
      printf '\n'
      ;;
  esac
}
ct_sccache_server_live() { [ "${FIXTURE_SERVER_LIVE:-1}" = 1 ]; }
pgrep() { [ "${FIXTURE_BUSY:-0}" = 1 ]; }
powershell.exe() { [ "${FIXTURE_BUSY:-0}" = 1 ]; }
ct_repo_root() { printf '%s\n' "$WORK"; }

(
  unset CT_SCCACHE_CONFIGURED SCCACHE_CACHE_SIZE
  export AWS_SESSION_TOKEN=fixture-old-session
  ct_configure_sccache
  [ "$SCCACHE_CACHE_SIZE" = 10G ]
  FIXTURE_BUSY=1 ct_configure_sccache
  [ "$(wc -l < "$WORK/events" | tr -d ' ')" = 2 ]
) > "$WORK/shared.log"
! grep -Eq 'fixture-access|fixture-secret|fixture-old-session' "$WORK/shared.log"
printf 'PASS shared configuration and nested reuse\n'

for missing in ACCESS SECRET; do
  printf 'Local disk: "fixture-disk"' > "$WORK/location"
  (
    unset CT_SCCACHE_CONFIGURED
    export SCCACHE_CACHE_SIZE=3G SCCACHE_BUCKET=stale SCCACHE_ENDPOINT=stale
    export "FIXTURE_$missing=0"
    ct_configure_sccache
    [ "$SCCACHE_CACHE_SIZE" = 3G ]
    [ "$CT_SCCACHE_STORAGE" = local ]
  ) > "$WORK/local.log"
  [ "$(grep -Ec '^sccache: local disk cache at .*shared credentials unavailable' "$WORK/local.log")" = 1 ]
  ! grep -Eq 'fixture-access|fixture-secret' "$WORK/local.log"
done
printf 'PASS missing either credential uses bounded local fallback\n'

(
  set -x
  ct_sccache_storage
  case "$-" in *x*) ;; *) exit 1 ;; esac
) > "$WORK/trace.log" 2>&1
! grep -Eq 'fixture-access|fixture-secret' "$WORK/trace.log"
printf 'PASS credential resolution suppresses tracing\n'

(
  FIXTURE_BUSY=1
  CT_SCCACHE_STORAGE=shared
  if ct_sccache_restart; then exit 1; else [ "$?" = 75 ]; fi
  FIXTURE_BUSY=0 FIXTURE_START_FAILURE=1
  if ct_sccache_restart; then exit 1; else [ "$?" = 2 ]; fi
) > "$WORK/refusals.log" 2>&1
printf 'PASS busy-machine and server-start refusals\n'

for checkout in /Users/Shared/domios-ci/work/codeterm /home/dev/domios-ci/work/codeterm /home/ci/domios-ci/work/codeterm /c/domios-ci/work/codeterm; do
  [ "$(ct_exceptional_target_dir "$checkout" /gateway fixture-id)" = "$checkout/target-check" ]
done
[ "$(ct_exceptional_target_dir /developer/worktree /gateway fixture-id)" = /gateway/targets/exceptional/fixture-id ]
printf 'PASS CI target paths and developer target isolation\n'

(
  rm -f "$WORK/events"
  export SCCACHE_CACHE_SIZE=10G
  curl() { printf 200; }
  printf 'Local disk: "fixture-disk"' > "$WORK/location"
  ct_sccache_status
  [ ! -e "$WORK/events" ]
) > "$WORK/status.log"
grep -Eq '^sccache: shared S3 cache at http://100.64.10.5:9000/sccache$' "$WORK/status.log"
grep -Eq '^running server: Cache location Local disk:' "$WORK/status.log"
grep -Eq '^shared endpoint health: 200$' "$WORK/status.log"
! grep -Eq 'fixture-access|fixture-secret' "$WORK/status.log"
printf 'PASS read-only cache status and endpoint health\n'

(
  export SCCACHE_CACHE_SIZE=10G FIXTURE_BUSY=1
  rm -f "$WORK/events"
  printf 's3, name: sccache, prefix: /' > "$WORK/location"
  unset CT_SCCACHE_CONFIGURED
  ct_configure_sccache
  [ ! -e "$WORK/events" ]
  printf 'Local disk: "fixture-disk"' > "$WORK/location"
  FIXTURE_ACCESS=0 ct_configure_sccache
  [ ! -e "$WORK/events" ]
  printf 's3, name: other-bucket, prefix: /' > "$WORK/location"
  if ct_configure_sccache; then exit 1; else [ "$?" = 75 ]; fi
  [ ! -e "$WORK/events" ]
  printf 's3, name: sccache, prefix: /other-prefix/' > "$WORK/location"
  SCCACHE_S3_KEY_PREFIX=desired-prefix/ ct_configure_sccache
  [ ! -e "$WORK/events" ]
) > "$WORK/idempotent.log" 2>&1
printf 'PASS busy matching servers are reused; wrong buckets refuse without stop\n'

(
  export SCCACHE_CACHE_SIZE=10G FIXTURE_BUSY=1
  rm -f "$WORK/events"
  printf 's3, name: sccache, prefix: /' > "$WORK/location"
  FIXTURE_ACCESS=0 ct_configure_sccache
  [ ! -e "$WORK/events" ]
  [ "$(cat "$WORK/location")" = 's3, name: sccache, prefix: /' ]
  FIXTURE_SECRET=0 ct_configure_sccache
  [ ! -e "$WORK/events" ]
  SCCACHE_S3_KEY_PREFIX=/ ct_configure_sccache
  [ ! -e "$WORK/events" ]
  printf 'Local disk: "fixture-disk"' > "$WORK/location"
  if ct_configure_sccache; then exit 1; else [ "$?" = 75 ]; fi
  [ ! -e "$WORK/events" ]
  FIXTURE_BUSY=0 ct_configure_sccache
  [ "$(wc -l < "$WORK/events" | tr -d ' ')" = 2 ]
  [ "$(cat "$WORK/location")" = 's3, name: sccache, prefix: /' ]
) > "$WORK/no-downgrade.log" 2>&1
[ "$(grep -Ec 'using running server credentials' "$WORK/no-downgrade.log")" = 2 ]
! grep -Eq '^sccache: local disk cache at' "$WORK/no-downgrade.log"
printf 'PASS unresolved keys never downgrade S3; local-to-S3 needs keys and idle\n'

(
  export FIXTURE_BUSY=1 CODETERM_SCCACHE_VERIFY_ONLY=1
  curl() { printf 200; }
  rm -f "$WORK/events"
  printf 's3, name: sccache, prefix: /' > "$WORK/location"
  ct_sccache_verify
  CODETERM_USE_SCCACHE=0 ct_configure_sccache
  [ ! -e "$WORK/events" ]
  for location in 'Local disk: "fixture-disk"' 's3, name: other-bucket, prefix: /' unavailable; do
    printf '%s' "$location" > "$WORK/location"
    if ct_sccache_verify; then exit 1; else [ "$?" = 2 ]; fi
    if ct_configure_sccache; then exit 1; else [ "$?" = 2 ]; fi
    [ ! -e "$WORK/events" ]
  done
  printf 's3, name: sccache, prefix: /' > "$WORK/location"
  if FIXTURE_SECRET=0 ct_sccache_verify; then exit 1; else [ "$?" = 2 ]; fi
  [ ! -e "$WORK/events" ]
  sccache() { return 127; }
  if ct_sccache_verify; then exit 1; else [ "$?" = 2 ]; fi
  command() { if [ "$1 $2" = '-v sccache' ]; then return 1; fi; builtin command "$@"; }
  if ct_configure_sccache; then exit 1; else [ "$?" = 2 ]; fi
) > "$WORK/verify.log" 2>&1
! grep -Eq 'fixture-access|fixture-secret|idle machine; Cargo' "$WORK/verify.log"
printf 'PASS CI verification never configures or refuses busy matching S3\n'

(
  export FIXTURE_BUSY=1 CODETERM_SCCACHE_VERIFY_ONLY=1
  curl() { printf 200; }
  ct_sccache_server_live() { [ -e "$WORK/location" ]; }
  rm -f "$WORK/events" "$WORK/location"
  if FIXTURE_SECRET=0 ct_sccache_verify; then exit 1; else [ "$?" = 2 ]; fi
  [ ! -e "$WORK/events" ]
  ct_configure_sccache
  [ "$(cat "$WORK/events")" = start ]
  [ "$(cat "$WORK/location")" = 's3, name: sccache, prefix: /' ]
  ct_sccache_verify
  [ "$(cat "$WORK/events")" = start ]
) > "$WORK/absent.log" 2>&1
! rg -q 'fixture-access|fixture-secret' "$WORK/absent.log"
printf 'PASS CI verification starts shared S3 only when no server is live\n'

(
  codeterm() { printf 'vault-call\n' >> "$WORK/events"; return 1; }
  rm -f "$WORK/events"
  export CODETERM_SCCACHE_CREDENTIAL_SOURCE=runner
  export AWS_ACCESS_KEY_ID=fixture-access AWS_SECRET_ACCESS_KEY=fixture-secret
  set -x
  ct_sccache_storage
  set +x
  [ "$CT_SCCACHE_STORAGE" = shared ]
  [ ! -e "$WORK/events" ]
  unset AWS_SECRET_ACCESS_KEY
  ct_sccache_storage
  [ "$CT_SCCACHE_STORAGE" = local ]
  [ ! -e "$WORK/events" ]
) > "$WORK/runner.log" 2>&1
! grep -Eq 'fixture-access|fixture-secret' "$WORK/runner.log"
printf 'PASS runner process credentials require both keys and never query vault\n'

(
  printf 's3, name: sccache, prefix: /' > "$WORK/location"
  curl() { printf 200; }
  FIXTURE_SERVER_LIVE=0
  if ct_sccache_verify; then exit 1; else [ "$?" = 2 ]; fi
) > "$WORK/offline.log" 2>&1
grep -Eq 'running server: cache location unavailable' "$WORK/offline.log"
printf 'PASS synthetic offline S3 stats cannot pass verification\n'
(
  FIXTURE_SERVER_LIVE=0
  if ct_sccache_stats; then exit 1; else [ "$?" = 2 ]; fi
  FIXTURE_SERVER_LIVE=1
  ct_repo_root() { printf '%s\n' "$ROOT"; }
  ct_sccache_stats
) > "$WORK/stats.log" 2>&1
grep -Eq '^sccache endpoint:' "$WORK/stats.log"
printf 'PASS stats refuses offline synthetic counters and names the endpoint\n'

(
  rm -f "$WORK/events" "$WORK/idle-scans"
  unset CT_SCCACHE_CONFIGURED
  export FIXTURE_BUSY=1 SCCACHE_CACHE_SIZE=10G
  mkdir -p "$WORK/gateway/lock"
  CT_GATEWAY_ACTIVE=1 CT_BUILD_JOB_ID=owned-cache CT_GATEWAY_PROCESS_START=fixture-start
  printf '%s\n' owned-cache > "$WORK/gateway/lock/job_id"
  printf '%s\n' "$$" > "$WORK/gateway/lock/pid"
  printf '%s\n' fixture-start > "$WORK/gateway/lock/start_token"
  pgrep() { echo scan >> "$WORK/idle-scans"; return 0; }
  powershell.exe() { echo scan >> "$WORK/idle-scans"; return 0; }
  source "$ROOT/scripts/build-gateway.sh"
  ct_gateway_state_tool() { [ "$1" = evidence-reset ]; }
  printf 's3, name: other-bucket, prefix: /' > "$WORK/location"
  ct_configure_sccache
  [ ! -e "$WORK/idle-scans" ]
  [ "$(wc -l < "$WORK/events" | tr -d ' ')" = 2 ]
  printf foreign-job > "$WORK/gateway/lock/job_id"
  printf 's3, name: other-bucket, prefix: /' > "$WORK/location"
  if ct_configure_sccache; then exit 1; else [ "$?" = 75 ]; fi
  [ "$(wc -l < "$WORK/events" | tr -d ' ')" = 2 ]
  [ -e "$WORK/idle-scans" ]
) > "$WORK/owned-busy.log" 2>&1
printf 'PASS exact gateway owner skips global idle scans; foreign marker refuses\n'
