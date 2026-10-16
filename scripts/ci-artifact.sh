#!/usr/bin/env bash
# Caller: CI ci-artifact-smoke.yml, tests.yml, rust-warnings-summary.yml, actions/rust-warnings
# CI artifacts in the shared MinIO ci-artifacts bucket instead of GitHub artifact storage.
#
#   ci-artifact.sh put <key> <file>
#   ci-artifact.sh get <key> <file>
#   ci-artifact.sh rm  <key>
#   ci-artifact.sh get-prefix <prefix> <dir>   (every object under prefix, by base name)
#
# Objects live in the ci-artifacts bucket (500 GB hard quota, 7-day expiry) next to the
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
  get-prefix)
    dir=${3:?missing directory}
    mkdir -p "$dir"
    keys=$(s3 "${endpoint%/}/$bucket?list-type=2&prefix=$key" | grep -o '<Key>[^<]*</Key>' | sed 's#</*Key>##g' || true)
    count=0
    for object in $keys; do
      s3 --output "$dir/${object##*/}" "${endpoint%/}/$bucket/$object"
      count=$((count + 1))
    done
    echo "ci-artifact: got $count object(s) under $bucket/$key into $dir"
    ;;
  *) echo "ci-artifact: unknown verb $verb" >&2; exit 2 ;;
esac
