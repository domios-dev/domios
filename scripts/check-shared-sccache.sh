#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/cache-policy.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

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
    --stop-server) printf 'stop\n' >> "$WORK/events" ;;
    --start-server)
      printf 'start\n' >> "$WORK/events"
      [ "${FIXTURE_START_FAILURE:-0}" = 0 ] || return 1
      if [ "$CT_SCCACHE_STORAGE" = shared ]; then
        [ "$AWS_ACCESS_KEY_ID" = fixture-access ]
        [ "$AWS_SECRET_ACCESS_KEY" = fixture-secret ]
        [ -z "${AWS_SESSION_TOKEN:-}" ]
        [ "$SCCACHE_ENDPOINT" = http://100.88.47.88:9000 ]
        [ "$SCCACHE_BUCKET:$SCCACHE_REGION:$SCCACHE_S3_USE_SSL" = sccache:us-east-1:false ]
      else
        [ -z "${SCCACHE_BUCKET:-}${SCCACHE_ENDPOINT:-}" ]
      fi
      ;;
    --version) printf 'sccache fixture\n' ;;
    --show-stats) printf 'Cache location fixture\n' ;;
  esac
}
pgrep() { [ "${FIXTURE_BUSY:-0}" = 1 ]; }
ct_repo_root() { printf '%s\n' "$WORK"; }

(
  unset CT_SCCACHE_CONFIGURED SCCACHE_CACHE_SIZE
  export AWS_SESSION_TOKEN=fixture-old-session
  ct_configure_sccache
  [ "$SCCACHE_CACHE_SIZE" = 10G ]
  ct_configure_sccache
  [ "$(wc -l < "$WORK/events" | tr -d ' ')" = 2 ]
) > "$WORK/shared.log"
! rg -q 'fixture-access|fixture-secret|fixture-old-session' "$WORK/shared.log"
printf 'PASS shared configuration and nested reuse\n'

for missing in ACCESS SECRET; do
  (
    unset CT_SCCACHE_CONFIGURED
    export SCCACHE_CACHE_SIZE=3G SCCACHE_BUCKET=stale SCCACHE_ENDPOINT=stale
    export "FIXTURE_$missing=0"
    ct_configure_sccache
    [ "$SCCACHE_CACHE_SIZE" = 3G ]
    [ "$CT_SCCACHE_STORAGE" = local ]
  ) > "$WORK/local.log"
  [ "$(rg -c '^sccache: local disk cache at .*shared credentials unavailable' "$WORK/local.log")" = 1 ]
  ! rg -q 'fixture-access|fixture-secret' "$WORK/local.log"
done
printf 'PASS missing either credential uses bounded local fallback\n'

(
  set -x
  ct_sccache_storage
  case "$-" in *x*) ;; *) exit 1 ;; esac
) > "$WORK/trace.log" 2>&1
! rg -q 'fixture-access|fixture-secret' "$WORK/trace.log"
printf 'PASS credential resolution suppresses tracing\n'

(
  FIXTURE_BUSY=1
  CT_SCCACHE_STORAGE=shared
  if ct_sccache_restart; then exit 1; else [ "$?" = 75 ]; fi
  FIXTURE_BUSY=0 FIXTURE_START_FAILURE=1
  if ct_sccache_restart; then exit 1; else [ "$?" = 2 ]; fi
) > "$WORK/refusals.log" 2>&1
printf 'PASS busy-machine and server-start refusals\n'

for checkout in /Users/Shared/domios-ci/work/codeterm /opt/domios-ci/work/codeterm /c/domios-ci/work/codeterm; do
  [ "$(ct_exceptional_target_dir "$checkout" /gateway fixture-id)" = "$checkout/target-check" ]
done
[ "$(ct_exceptional_target_dir /developer/worktree /gateway fixture-id)" = /gateway/targets/exceptional/fixture-id ]
printf 'PASS CI target paths and developer target isolation\n'

(
  rm -f "$WORK/events"
  export SCCACHE_CACHE_SIZE=10G
  curl() { printf 200; }
  ct_sccache_status
  [ ! -e "$WORK/events" ]
) > "$WORK/status.log"
rg -q '^sccache: shared S3 cache at http://100.88.47.88:9000/sccache$' "$WORK/status.log"
rg -q '^running server: Cache location fixture$' "$WORK/status.log"
rg -q '^shared endpoint health: 200$' "$WORK/status.log"
! rg -q 'fixture-access|fixture-secret' "$WORK/status.log"
printf 'PASS read-only cache status and endpoint health\n'
