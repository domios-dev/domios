#!/usr/bin/env bash
# Caller: CI ci-artifact-smoke.yml, frontend-release.yml, tests-fast.yml, tests-rust.yml, rust-warnings-summary.yml, release-manifest-dry-run.yml, actions/rust-warnings; scripts release-dry-run-manifest.mjs
# CI artifacts in the shared MinIO ci-artifacts bucket instead of GitHub artifact storage.
#
#   ci-artifact.sh put <key> <file>
#   ci-artifact.sh get <key> <file>
#   ci-artifact.sh rm  <key>
#   ci-artifact.sh get-prefix <prefix> <dir>   (every object under prefix, by base name)
#
# Objects live in the ci-artifacts bucket (500 GB hard quota, 7-day expiry) next to the
# sccache bucket, signed with the same runner keys; the keys reach curl through stdin, never argv.
#
# In a GitHub Actions job whose store is unreachable, put spools the file under
# $RUNNER_TEMP/ci-artifact-spool/<key> with a warning; the job's ci-artifact-spool action uploads
# it as a run artifact, and get/get-prefix read keys "<run-id>/..." back from that run's artifacts.
# CI_ARTIFACT_SOURCES, when set, names a file that receives "<key>\t<location>" per object moved.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/ci-store.sh"
endpoint=${SCCACHE_ENDPOINT:-$CT_CI_STORE_ENDPOINT}
bucket=${CI_ARTIFACT_BUCKET:-ci-artifacts}
region=${SCCACHE_REGION:-us-east-1}
verb=${1:?usage: ci-artifact.sh put|get|rm <key> [file]}
key=${2:?missing object key}
case "$key" in
  *..*|/*|*' '*) echo "ci-artifact: unsafe key: $key" >&2; exit 2 ;;
esac
url="${endpoint%/}/$bucket/$key"

record() {
  [ -z "${CI_ARTIFACT_SOURCES:-}" ] || printf '%s\t%s\n' "$1" "$2" >> "$CI_ARTIFACT_SOURCES"
}

store_down() {
  [ "${CI_ARTIFACT_STORAGE:-}" != github ] || return 0
  [ -n "${GITHUB_ACTIONS:-}" ] && ! CT_CI_STORE_ENDPOINT="$endpoint" ct_ci_store_reachable
}

spool_put() {
  local spool="${RUNNER_TEMP:?RUNNER_TEMP is not set}/ci-artifact-spool" name
  mkdir -p "$spool/$(dirname "$key")"
  [ -f "$spool/.name" ] || printf 'ci-artifact-spool-%s-%s-%s\n' "${GITHUB_JOB:-job}" "$(date +%s)" "$$$RANDOM" > "$spool/.name"
  name=$(cat "$spool/.name")
  cp "$1" "$spool/$key"
  echo "::warning::private store $endpoint is unreachable; $key is retained in run artifact $name"
  record "$key" "run-artifact:$name/$key"
}

# Downloads every spooled artifact of the key's run once per job.
fallback_cache() {
  local run="${key%%/*}" cache
  case "$run" in ''|*[!0-9]*) echo "ci-artifact: key $key names no run id for the run-artifact fallback" >&2; return 2 ;; esac
  cache="${RUNNER_TEMP:?RUNNER_TEMP is not set}/ci-artifact-fallback/$run"
  if [ ! -d "$cache" ]; then
    export GH_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
    [ -n "$GH_TOKEN" ] || { echo "ci-artifact: store unreachable and no GH_TOKEN for the run-artifact fallback" >&2; return 2; }
    rm -rf "$cache.part"
    mkdir -p "$cache.part"
    gh run download "$run" --repo "${GITHUB_REPOSITORY:?}" --pattern 'ci-artifact-spool-*' --dir "$cache.part" >&2
    mv "$cache.part" "$cache"
  fi
  printf '%s\n' "$cache"
}

# Prints "<artifact-name>\t<path>" for the spooled copy of $1 (a key or a prefix ending in /).
fallback_locate() {
  local cache="$1" wanted="$2" candidate
  if [ -e "$cache/$wanted" ]; then
    printf 'run-artifact\t%s\n' "$cache/$wanted"
    return 0
  fi
  for candidate in "$cache"/*/"$wanted"; do
    [ -e "$candidate" ] || continue
    printf '%s\t%s\n' "$(basename "${candidate%/"$wanted"}")" "$candidate"
    return 0
  done
  return 1
}

s3() {
  : "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is not set}"
  : "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is not set}"
  printf 'user = "%s:%s"\n' "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" |
    curl --config - --silent --show-error --fail --retry 2 \
      --aws-sigv4 "aws:amz:$region:s3" "$@"
}

case "$verb" in
  put|get|get-prefix|rm) ;;
  *) echo "ci-artifact: unknown verb $verb" >&2; exit 2 ;;
esac

if store_down; then
  case "$verb" in
    put) spool_put "${3:?missing file}" ;;
    get)
      file=${3:?missing file}
      cache=$(fallback_cache)
      found=$(fallback_locate "$cache" "$key") || { echo "ci-artifact: $key is in no run artifact of run ${key%%/*}" >&2; exit 1; }
      cp "${found#*$'\t'}" "$file"
      record "$key" "run-artifact:${found%%$'\t'*}/$key"
      echo "ci-artifact: got $key from run artifact ${found%%$'\t'*} ($(wc -c < "$file") bytes)"
      ;;
    get-prefix)
      dir=${3:?missing directory}
      mkdir -p "$dir"
      cache=$(fallback_cache)
      count=0
      if found=$(fallback_locate "$cache" "$key"); then
        for object in "${found#*$'\t'}"/*; do
          [ -f "$object" ] || continue
          cp "$object" "$dir/${object##*/}"
          record "$key${object##*/}" "run-artifact:${found%%$'\t'*}/$key${object##*/}"
          count=$((count + 1))
        done
      fi
      echo "ci-artifact: got $count object(s) under $key from the run artifacts into $dir"
      ;;
    rm) echo "::warning::private store $endpoint is unreachable; $key was not removed" ;;
  esac
  exit 0
fi

case "$verb" in
  put)
    file=${3:?missing file}
    s3 --upload-file "$file" "$url" >/dev/null
    record "$key" "private-store:$bucket/$key"
    echo "ci-artifact: put $bucket/$key ($(wc -c < "$file") bytes)"
    ;;
  get)
    file=${3:?missing file}
    s3 --output "$file" "$url"
    record "$key" "private-store:$bucket/$key"
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
      record "$object" "private-store:$bucket/$object"
      count=$((count + 1))
    done
    echo "ci-artifact: got $count object(s) under $bucket/$key into $dir"
    ;;
esac
