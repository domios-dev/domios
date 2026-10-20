#!/usr/bin/env bash
# Caller: scripts cache-policy.sh, ci-artifact.sh, check-shared-sccache.sh; skill build, github-release
# The fleet's private object store (MinIO) holding the shared sccache bucket and CI evidence.
# A replica takes over by changing this one value or setting CODETERM_CI_STORE_ENDPOINT.
CT_CI_STORE_ENDPOINT="${CODETERM_CI_STORE_ENDPOINT:-http://100.64.10.5:9000}"

ct_ci_store_reachable() {
  curl --silent --fail --output /dev/null --connect-timeout 5 --max-time 10 \
    "${CT_CI_STORE_ENDPOINT%/}/minio/health/live" 2>/dev/null
}
