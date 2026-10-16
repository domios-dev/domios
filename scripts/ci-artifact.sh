#!/usr/bin/env bash
# Caller: CI ci-artifact-smoke.yml
# CI artifacts in the shared MinIO bucket instead of GitHub artifact storage.
#
#   ci-artifact.sh put <key> <file>
#   ci-artifact.sh get <key> <file>
#   ci-artifact.sh rm  <key>
#
# Objects live under ci-artifacts/ in $SCCACHE_BUCKET at $SCCACHE_ENDPOINT, signed
# with the runner's AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY; the keys reach curl
# through stdin, never argv.
set -euo pipefail

endpoint=${SCCACHE_ENDPOINT:-http://100.88.47.88:9000}
bucket=${SCCACHE_BUCKET:-sccache}
region=${SCCACHE_REGION:-us-east-1}
verb=${1:?usage: ci-artifact.sh put|get|rm <key> [file]}
key=${2:?missing object key}
case "$key" in
  *..*|/*|*' '*) echo "ci-artifact: unsafe key: $key" >&2; exit 2 ;;
esac
: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is not set}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is not set}"
url="${endpoint%/}/$bucket/ci-artifacts/$key"

s3() {
  printf 'user = "%s:%s"\n' "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" |
    curl --config - --silent --show-error --fail --retry 2 \
      --aws-sigv4 "aws:amz:$region:s3" "$@"
}

case "$verb" in
  put)
    file=${3:?missing file}
    s3 --upload-file "$file" "$url" >/dev/null
    echo "ci-artifact: put ci-artifacts/$key ($(wc -c < "$file") bytes)"
    ;;
  get)
    file=${3:?missing file}
    s3 --output "$file" "$url"
    echo "ci-artifact: got ci-artifacts/$key ($(wc -c < "$file") bytes)"
    ;;
  rm)
    s3 --request DELETE "$url" >/dev/null
    echo "ci-artifact: removed ci-artifacts/$key"
    ;;
  *) echo "ci-artifact: unknown verb $verb" >&2; exit 2 ;;
esac
