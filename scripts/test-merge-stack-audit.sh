#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

git -C "$TMP" init -q
git -C "$TMP" config user.email test@example.invalid
git -C "$TMP" config user.name "Test User"

printf 'base\n' > "$TMP/file.txt"
git -C "$TMP" add file.txt
git -C "$TMP" commit -q -m base
target="$(git -C "$TMP" rev-parse HEAD)"

git -C "$TMP" checkout -q -b merged-lane
printf 'merged\n' > "$TMP/merged.txt"
git -C "$TMP" add merged.txt
git -C "$TMP" commit -q -m merged-lane
merged_lane="$(git -C "$TMP" rev-parse HEAD)"

git -C "$TMP" checkout -q -b target "$target"
git -C "$TMP" merge -q --no-ff merged-lane -m "merge lane"
target_with_merge="$(git -C "$TMP" rev-parse HEAD)"

git -C "$TMP" checkout -q -b unlanded-lane "$target"
printf 'unlanded\n' > "$TMP/unlanded.txt"
git -C "$TMP" add unlanded.txt
git -C "$TMP" commit -q -m unlanded-lane
unlanded_lane="$(git -C "$TMP" rev-parse HEAD)"

printf '%s\n%s' "$merged_lane" "$unlanded_lane" > "$TMP/refs-no-final-lf.txt"
printf '%s\r\n' "$merged_lane" > "$TMP/refs-crlf.txt"

set +e
output="$(cd "$TMP" && "$ROOT/scripts/merge-stack-audit.sh" \
  --target "$target_with_merge" \
  --refs-file "$TMP/refs-no-final-lf.txt" 2>&1)"
status=$?
set -e

grep -F "MERGED	$merged_lane" >/dev/null <<<"$output"
grep -F "UNLANDED	$unlanded_lane" >/dev/null <<<"$output"
grep -F "unlanded=1" >/dev/null <<<"$output"

if [ "$status" -eq 0 ]; then
  printf 'expected merge-stack-audit to fail on final unlanded ref without LF\n' >&2
  exit 1
fi

crlf_output="$(cd "$TMP" && "$ROOT/scripts/merge-stack-audit.sh" \
  --target "$target_with_merge" \
  --refs-file "$TMP/refs-crlf.txt" 2>&1)"

grep -F "MERGED	$merged_lane" >/dev/null <<<"$crlf_output"
grep -F "missing=0" >/dev/null <<<"$crlf_output"

printf 'merge-stack-audit unterminated refs-file test passed\n'
