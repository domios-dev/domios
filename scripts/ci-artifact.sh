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
  # One backend for the entire Actions run, across hosted and private runners.
  if [ "${GITHUB_ACTIONS:-}" = true ]; then
    case "$key" in
      [0-9]*/rust-receipt/*|[0-9]*/rust-warnings/*|[0-9]*/rust-warnings-*/*|[0-9]*/frontend-dist/*) return 0 ;;
    esac
  fi
  [ "${CI_ARTIFACT_STORAGE:-}" != github ] || return 0
  [ -n "${GITHUB_ACTIONS:-}" ] && ! CT_CI_STORE_ENDPOINT="$endpoint" ct_ci_store_reachable
}

spool_put() {
  local spool="${RUNNER_TEMP:?RUNNER_TEMP is not set}/ci-artifact-spool" name
  mkdir -p "$spool/$(dirname "$key")"
  [ -f "$spool/.name" ] || printf 'ci-artifact-spool-%s-%s-%s\n' "${GITHUB_JOB:-job}" "$(date +%s)" "$$$RANDOM" > "$spool/.name"
  name=$(cat "$spool/.name")
  cp "$1" "$spool/$key"
  echo "::warning::artifact backend selected; $key is retained in run artifact $name"
  record "$key" "run-artifact:$name/$key"
}

# Downloads every spooled artifact of the key's run once per job.
fallback_cache() {
  local run="${key%%/*}" cache
  case "$run" in ''|*[!0-9]*) echo "ci-artifact: key $key names no run id for the run-artifact fallback" >&2; return 2 ;; esac
  cache="${RUNNER_TEMP:?RUNNER_TEMP is not set}/ci-artifact-fallback/$run"
  if [ -d "$RUNNER_TEMP/ci-artifact-spool" ]; then mkdir -p "$cache"; fi
  if [ ! -d "$cache" ]; then
    echo "ci-artifact: workflow must download ci-artifact-spool artifacts for run $run before reading fallback evidence" >&2
    return 2
  fi
  printf '%s\n' "$cache"
}

# Prints "<artifact-name>\t<path>" for the spooled copy of $1 (a key or a prefix ending in /).
fallback_locate() {
  local cache="$1" wanted="$2" candidate
  if [ -e "$cache/$wanted" ]; then
    printf 'run-artifact\t%s\n' "$cache/$wanted"
  fi
  for candidate in "$cache"/*/"$wanted" "$RUNNER_TEMP/ci-artifact-spool/$wanted"; do
    [ -e "$candidate" ] || continue
    printf '%s\t%s\n' "$(basename "${candidate%/"$wanted"}")" "$candidate"
  done
  return 0
}

s3() {
  : "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is not set}"
  : "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is not set}"
  printf 'user = "%s:%s"\n' "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" |
    curl --config - --silent --show-error --fail --retry 2 \
      --aws-sigv4 "aws:amz:$region:s3" "$@"
}

# Large objects move in parallel parts: one TCP stream from a far runner cannot fill the store's link.
part_bytes=$((32 * 1024 * 1024))
part_streams=8

multipart_put() {
  local file="$1" size parts work id n etag xml
  size=$(wc -c < "$file")
  parts=$(( (size + part_bytes - 1) / part_bytes ))
  work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ci-artifact-parts.XXXXXX")
  id=$(s3 --request POST "$url?uploads" | grep -o '<UploadId>[^<]*</UploadId>' | sed 's#</*UploadId>##g')
  [ -n "$id" ] || { rm -rf "$work"; echo "ci-artifact: store refused a multipart upload for $key" >&2; return 1; }
  for n in $(seq 1 "$parts"); do
    while [ "$(jobs -rp | wc -l)" -ge "$part_streams" ]; do wait -n || true; done
    (
      dd if="$file" of="$work/$n.part" bs="$part_bytes" skip=$((n - 1)) count=1 2>/dev/null
      s3 --request PUT --upload-file "$work/$n.part" --dump-header "$work/$n.headers" --output /dev/null \
        "$url?partNumber=$n&uploadId=$id" && rm -f "$work/$n.part"
    ) &
  done
  wait
  xml='<CompleteMultipartUpload>'
  for n in $(seq 1 "$parts"); do
    etag=$(grep -i '^etag:' "$work/$n.headers" 2>/dev/null | tr -d '\r' | sed 's/^[Ee][Tt][Aa][Gg]: *//')
    if [ -z "$etag" ]; then
      s3 --request DELETE "$url?uploadId=$id" >/dev/null || true
      rm -rf "$work"
      echo "ci-artifact: part $n of $key did not upload" >&2
      return 1
    fi
    xml+="<Part><PartNumber>$n</PartNumber><ETag>$etag</ETag></Part>"
  done
  xml+='</CompleteMultipartUpload>'
  printf '%s' "$xml" > "$work/complete.xml"
  s3 --request POST --header 'Content-Type: application/xml' --data-binary "@$work/complete.xml" \
    "$url?uploadId=$id" | grep -q '<CompleteMultipartUploadResult' || {
    s3 --request DELETE "$url?uploadId=$id" >/dev/null || true
    rm -rf "$work"
    echo "ci-artifact: store did not complete the multipart upload of $key" >&2
    return 1
  }
  rm -rf "$work"
}

ranged_get() {
  local file="$1" size parts work n first last
  size=$(s3 --head "$url" | tr -d '\r' | grep -i '^content-length:' | awk '{print $2}')
  if [ -z "$size" ] || [ "$size" -le "$part_bytes" ]; then
    s3 --output "$file" "$url"
    return
  fi
  parts=$(( (size + part_bytes - 1) / part_bytes ))
  work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ci-artifact-parts.XXXXXX")
  for n in $(seq 1 "$parts"); do
    while [ "$(jobs -rp | wc -l)" -ge "$part_streams" ]; do wait -n || true; done
    first=$(( (n - 1) * part_bytes ))
    last=$(( n * part_bytes - 1 ))
    [ "$last" -lt "$size" ] || last=$((size - 1))
    s3 --range "$first-$last" --output "$work/$n.part" "$url" &
  done
  wait
  : > "$file"
  for n in $(seq 1 "$parts"); do
    [ -f "$work/$n.part" ] || { rm -rf "$work"; echo "ci-artifact: part $n of $key did not download" >&2; return 1; }
    cat "$work/$n.part" >> "$file"
  done
  rm -rf "$work"
  [ "$(wc -c < "$file")" -eq "$size" ] || { echo "ci-artifact: $key arrived truncated" >&2; return 1; }
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
      found_any=0
      while IFS=$'\t' read -r artifact object; do
        [ -n "$object" ] || continue
        if [ "$found_any" = 1 ]; then
          cmp -s "$object" "$file" || { echo "ci-artifact: conflicting copies of $key" >&2; exit 1; }
        else
          cp "$object" "$file"
          found_any=1
        fi
        record "$key" "run-artifact:$artifact/$key"
      done < <(fallback_locate "$cache" "$key")
      [ "$found_any" = 1 ] || { echo "ci-artifact: $key is in no run artifact of run ${key%%/*}" >&2; exit 1; }
      echo "ci-artifact: got $key from run artifacts"
      ;;
    get-prefix)
      dir=${3:?missing directory}
      mkdir -p "$dir"
      cache=$(fallback_cache)
      count=0
      while IFS=$'\t' read -r artifact directory; do
        [ -n "$directory" ] || continue
        for object in "$directory"/*; do
          [ -f "$object" ] || continue
          target="$dir/${object##*/}"
          if [ -f "$target" ]; then
            cmp -s "$object" "$target" || { echo "ci-artifact: conflicting copies of $key${object##*/}" >&2; exit 1; }
          else
            cp "$object" "$target"
          fi
          count=$((count + 1))
          record "$key${object##*/}" "run-artifact:$artifact/$key${object##*/}"
        done
      done < <(fallback_locate "$cache" "$key")
      [ "$count" -gt 0 ] || { echo "ci-artifact: $key is in no run artifact" >&2; exit 1; }
      echo "ci-artifact: got $count object(s) under $key from the run artifacts into $dir"
      ;;
    rm) echo "::warning::private store $endpoint is unreachable; $key was not removed" ;;
  esac
  exit 0
fi

case "$verb" in
  put)
    file=${3:?missing file}
    if [ "$(wc -c < "$file")" -gt "$part_bytes" ]; then
      multipart_put "$file"
    else
      s3 --upload-file "$file" "$url" >/dev/null
    fi
    record "$key" "private-store:$bucket/$key"
    echo "ci-artifact: put $bucket/$key ($(wc -c < "$file") bytes)"
    ;;
  get)
    file=${3:?missing file}
    ranged_get "$file"
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
