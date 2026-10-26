#!/usr/bin/env bash
# Caller: scripts cache-policy.sh, ci-artifact.sh, check-shared-sccache.sh; skill build, github-release
# The fleet's private object store (MinIO) holding the shared sccache bucket and CI evidence.
# CI jobs receive both addresses as secrets: our runners use the tailnet one, GitHub-hosted runners the public
# HTTPS edge. CODETERM_CI_STORE_ENDPOINT overrides both. The address never lives in the repository.
if [ -n "${CODETERM_CI_STORE_ENDPOINT:-}" ]; then
  CT_CI_STORE_ENDPOINT="$CODETERM_CI_STORE_ENDPOINT"
elif [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ]; then
  CT_CI_STORE_ENDPOINT="${CI_STORE_PUBLIC_ENDPOINT:-}"
else
  CT_CI_STORE_ENDPOINT="${CI_STORE_TAILNET_ENDPOINT:-}"
fi

ct_ci_store_reachable() {
  [ -n "$CT_CI_STORE_ENDPOINT" ] || return 1
  curl --silent --fail --output /dev/null --connect-timeout 5 --max-time 10 \
    "${CT_CI_STORE_ENDPOINT%/}/minio/health/live" 2>/dev/null
}
