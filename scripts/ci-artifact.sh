#!/usr/bin/env bash
# Caller: CI ci-artifact-smoke.yml
# CI artifacts in the shared MinIO ci-artifacts bucket instead of GitHub artifact storage.
#
#   ci-artifact.sh put <key> <file>
#   ci-artifact.sh get <key> <file>
#   ci-artifact.sh rm  <key>
#
# Objects live in the ci-artifacts bucket (50 GB hard quota, 7-day expiry) next to the
# sccache bucket, signed with the same runner keys; the keys reach curl through stdin, never argv.
set -euo pipefail

endpoint=${SCCACHE_ENDPOINT:-http://100.64.10.5:9000}
bucket=${CI_ARTIFACT_BUCKET:-ci-artifacts}
region=${SCCACHE_REGION:-us-east-1}
verb=${1:?usage: ci-artifact.sh put|get|rm <key> [file]}
key=${2:?missing object key}
case "$key" in
  *..*|/*|*' '*) echo "ci-artifact: unsafe key: $key" >&2; exit 2 ;;
esac
: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is not set}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is not set}"
url="${endpoint%/}/$bucket/$key"

s3() {
  printf 'user = "%s:%s"\n' "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" |
    curl --config - --silent --show-error --fail --retry 2 \
      --aws-sigv4 "aws:amz:$region:s3" "$@"
}

case "$verb" in
  put)
    file=${3:?missing file}
    s3 --upload-file "$file" "$url" >/dev/null
    echo "ci-artifact: put $bucket/$key ($(wc -c < "$file") bytes)"
    ;;
  get)
    file=${3:?missing file}
    s3 --output "$file" "$url"
    echo "ci-artifact: got $bucket/$key ($(wc -c < "$file") bytes)"
    ;;
  rm)
    s3 --request DELETE "$url" >/dev/null
    echo "ci-artifact: removed $bucket/$key"
    ;;
  *) echo "ci-artifact: unknown verb $verb" >&2; exit 2 ;;
esac
