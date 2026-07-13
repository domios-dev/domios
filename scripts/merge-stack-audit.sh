#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/merge-stack-audit.sh [--target REF] [--refs-file FILE] [--report-only] [REF...]

Classify accepted lane refs against a release candidate:
  MERGED              ref is an ancestor of the target
  PATCH_LANDED_STALE  patch-equivalent by git cherry, but the lane ref still exists
  UNLANDED            contains commits not present in target

The gate fails on PATCH_LANDED_STALE and UNLANDED. Patch equivalence is not
ancestry proof; clean up only after a real merge or an explicit superseded record.
EOF
}

target="HEAD"
refs_file=""
report_only=0
refs=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      target="${2:?missing --target value}"
      shift 2
      ;;
    --refs-file)
      refs_file="${2:?missing --refs-file value}"
      shift 2
      ;;
    --report-only)
      report_only=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      refs+=("$@")
      break
      ;;
    -*)
      echo "merge-stack-audit: unknown option $1" >&2
      usage >&2
      exit 2
      ;;
    *)
      refs+=("$1")
      shift
      ;;
  esac
done

if [ -n "$refs_file" ]; then
  while IFS= read -r ref || [ -n "$ref" ]; do
    ref=${ref%$'\r'}
    case "$ref" in
      ""|\#*) continue ;;
      *) refs+=("$ref") ;;
    esac
  done < "$refs_file"
fi

if [ "${#refs[@]}" -eq 0 ]; then
  echo "merge-stack-audit: provide accepted lane refs or --refs-file" >&2
  usage >&2
  exit 2
fi

target_sha="$(git rev-parse --verify "$target^{commit}")"

merged=0
patch_landed=0
unlanded=0
missing=0

printf 'target\t%s\t%s\n' "$target" "$target_sha"

for ref in "${refs[@]}"; do
  if ! ref_sha="$(git rev-parse --verify "$ref^{commit}" 2>/dev/null)"; then
    printf 'MISSING\t%s\t-\n' "$ref"
    missing=$((missing + 1))
    continue
  fi

  if git merge-base --is-ancestor "$ref_sha" "$target_sha"; then
    printf 'MERGED\t%s\t%s\n' "$ref" "$ref_sha"
    merged=$((merged + 1))
    continue
  fi

  cherry="$(git cherry "$target_sha" "$ref_sha")"
  if ! printf '%s\n' "$cherry" | grep -q '^+'; then
    printf 'PATCH_LANDED_STALE\t%s\t%s\n' "$ref" "$ref_sha"
    patch_landed=$((patch_landed + 1))
    continue
  fi

  printf 'UNLANDED\t%s\t%s\n' "$ref" "$ref_sha"
  unlanded=$((unlanded + 1))
done

printf 'summary\tmerged=%s\tpatch_landed_stale=%s\tunlanded=%s\tmissing=%s\n' \
  "$merged" "$patch_landed" "$unlanded" "$missing"

if [ "$report_only" = "1" ]; then
  exit 0
fi

if [ "$patch_landed" -gt 0 ] || [ "$unlanded" -gt 0 ] || [ "$missing" -gt 0 ]; then
  exit 1
fi
