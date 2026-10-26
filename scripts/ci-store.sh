#!/usr/bin/env bash
# Caller: scripts cache-policy.sh, ci-artifact.sh, check-shared-sccache.sh; skill build, github-release
# The fleet's private object store (MinIO) holding the shared sccache bucket and CI evidence.
# CI jobs receive both addresses as secrets. Our runners prefer the store's LAN address and fall back to the public
# HTTPS edge when it does not answer; GitHub-hosted runners use the edge. CODETERM_CI_STORE_ENDPOINT overrides both.
# The address never lives in the repository.

ct_ci_store_answers() {
  [ -n "$1" ] && curl --silent --fail --output /dev/null --connect-timeout 2 --max-time 5 \
    "${1%/}/minio/health/live" 2>/dev/null
}

ct_ci_store_select() {
  local candidate
  if [ -n "${CODETERM_CI_STORE_ENDPOINT:-}" ]; then
    CT_CI_STORE_ENDPOINT="$CODETERM_CI_STORE_ENDPOINT"
    return
  fi
  if [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ]; then
    CT_CI_STORE_ENDPOINT="${CI_STORE_PUBLIC_ENDPOINT:-}"
    return
  fi
  for candidate in "${CI_STORE_LAN_ENDPOINT:-}" "${CI_STORE_PUBLIC_ENDPOINT:-}"; do
    if ct_ci_store_answers "$candidate"; then
      CT_CI_STORE_ENDPOINT="$candidate"
      return
    fi
  done
  # Nothing answers: keep the preferred address so reachability checks fail and CI degrades to a job-local cache.
  CT_CI_STORE_ENDPOINT="${CI_STORE_LAN_ENDPOINT:-${CI_STORE_PUBLIC_ENDPOINT:-}}"
}
ct_ci_store_select

ct_ci_store_reachable() {
  [ -n "$CT_CI_STORE_ENDPOINT" ] || return 1
  curl --silent --fail --output /dev/null --connect-timeout 5 --max-time 10 \
    "${CT_CI_STORE_ENDPOINT%/}/minio/health/live" 2>/dev/null
}
