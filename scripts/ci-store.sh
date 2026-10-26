#!/usr/bin/env bash
# Caller: scripts cache-policy.sh, ci-artifact.sh, check-shared-sccache.sh; skill build, github-release
# The fleet's private object store (MinIO) holding the shared sccache bucket and CI evidence.
# Our machines set CODETERM_CI_STORE_ENDPOINT (tailnet) in their runner .env; GitHub-hosted push jobs receive the
# public HTTPS edge as CI_STORE_PUBLIC_ENDPOINT. The address never lives in the repository.
CT_CI_STORE_ENDPOINT="${CODETERM_CI_STORE_ENDPOINT:-${CI_STORE_PUBLIC_ENDPOINT:-}}"

ct_ci_store_reachable() {
  [ -n "$CT_CI_STORE_ENDPOINT" ] || return 1
  curl --silent --fail --output /dev/null --connect-timeout 5 --max-time 10 \
    "${CT_CI_STORE_ENDPOINT%/}/minio/health/live" 2>/dev/null
}
