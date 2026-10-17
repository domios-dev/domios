#!/usr/bin/env bash
set -euo pipefail

source scripts/firebase-build-config.sh
capture="$(mktemp)"
trap 'rm -f "$capture"' EXIT

fail() {
  printf '%s\n' 'Firebase build config test failed.' >&2
  exit 1
}

codeterm() {
  [ "$#" = 5 ] && [ "$1" = mem ] && [ "$2" = secret ] && [ "$3" = get ] && [ "$4" = --name ] || return 90
  case "${SECRET_MODE:-present}:$5" in
    present:DOMIOS_FIREBASE_API_KEY) printf '%s' 'fixture-api-key' ;;
    present:DOMIOS_FIREBASE_PROJECT_ID) printf '%s' 'fixture-project' ;;
    present:DOMIOS_FIREBASE_APP_ID) printf '%s' 'fixture-app' ;;
    present:DOMIOS_ACCOUNT_REDEEM_URL) printf '%s' 'https://fixture.invalid/redeem' ;;
    *) return 1 ;;
  esac
}

assert_secret_environment() {
  [ "$DOMIOS_FIREBASE_API_KEY" = 'fixture-api-key' ] &&
    [ "$DOMIOS_FIREBASE_PROJECT_ID" = 'fixture-project' ] &&
    [ "$DOMIOS_FIREBASE_APP_ID" = 'fixture-app' ] &&
    [ "$DOMIOS_ACCOUNT_REDEEM_URL" = 'https://fixture.invalid/redeem' ] || return 1
}

assert_empty_environment() {
  [ -z "$DOMIOS_FIREBASE_API_KEY" ] &&
    [ -z "$DOMIOS_FIREBASE_PROJECT_ID" ] &&
    [ -z "$DOMIOS_FIREBASE_APP_ID" ] &&
    [ -z "$DOMIOS_ACCOUNT_REDEEM_URL" ] || return 1
}

run_with_trace() {
  local status=0
  set -x
  ct_with_firebase_build_config "$@" >"$capture" 2>&1 || status=$?
  set +x
  return "$status"
}

SECRET_MODE=present
unset DOMIOS_FIREBASE_API_KEY DOMIOS_FIREBASE_PROJECT_ID DOMIOS_FIREBASE_APP_ID DOMIOS_ACCOUNT_REDEEM_URL
run_with_trace assert_secret_environment || fail
present_output="$(<"$capture")"
case "$present_output" in
  *'Firebase config: cloud-auth enabled'*) ;;
  *) fail ;;
esac
case "$present_output" in
  *fixture-api-key*|*fixture-project*|*fixture-app*|*fixture.invalid*) fail ;;
esac
[ "${DOMIOS_FIREBASE_API_KEY+x}${DOMIOS_FIREBASE_PROJECT_ID+x}${DOMIOS_FIREBASE_APP_ID+x}${DOMIOS_ACCOUNT_REDEEM_URL+x}" = '' ] || fail

SECRET_MODE=absent
DOMIOS_FIREBASE_API_KEY='parent-partial-value'
unset DOMIOS_FIREBASE_PROJECT_ID DOMIOS_FIREBASE_APP_ID DOMIOS_ACCOUNT_REDEEM_URL
ct_with_firebase_build_config assert_empty_environment >"$capture" 2>&1 || fail
absent_output="$(<"$capture")"
case "$absent_output" in
  *'Firebase config: local-only'*) ;;
  *) fail ;;
esac
case "$absent_output" in
  *parent-partial-value*|*fixture-api-key*|*fixture-project*|*fixture-app*|*fixture.invalid*) fail ;;
esac
[ "$DOMIOS_FIREBASE_API_KEY" = 'parent-partial-value' ] || fail
[ -z "${DOMIOS_FIREBASE_PROJECT_ID+x}${DOMIOS_FIREBASE_APP_ID+x}${DOMIOS_ACCOUNT_REDEEM_URL+x}" ] || fail

printf '%s\n' 'Firebase build config env resolution passed.'
