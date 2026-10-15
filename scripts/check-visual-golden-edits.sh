#!/usr/bin/env bash
# Caller: build.sh script-guards
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

REVIEW="tests/playwright/visual-goldens-reviewed-diff.md"
SNAP_DIR="tests/playwright/suites/13-canvas/visual-regression.spec.ts-snapshots"

base="${VISUAL_GOLDEN_BASE:-}"
if [ -z "$base" ]; then
  if git rev-parse --verify @{upstream} >/dev/null 2>&1; then
    base="$(git merge-base HEAD @{upstream})" || { echo "no merge base with @{upstream}; set VISUAL_GOLDEN_BASE" >&2; exit 1; }
  elif git rev-parse --verify origin/master >/dev/null 2>&1; then
    base="$(git merge-base HEAD origin/master)" || { echo "no merge base with origin/master; set VISUAL_GOLDEN_BASE" >&2; exit 1; }
  else
    base="HEAD"
  fi
fi

changed="$(git diff --name-only "$base"...HEAD -- "$SNAP_DIR/*.png" "$SNAP_DIR" || true)"
working="$(git status --short -- "$SNAP_DIR" | awk '{print $2}' || true)"
all_changed="$(printf "%s\n%s\n" "$changed" "$working" | sed '/^$/d' | sort -u)"

if [ -z "$all_changed" ]; then
  exit 0
fi

review_changed="$(git diff --name-only "$base"...HEAD -- "$REVIEW" || true)"
review_working="$(git status --short -- "$REVIEW" | awk '{print $2}' || true)"
if [ -n "$review_changed$review_working" ] && [ -s "$REVIEW" ]; then
  exit 0
fi

echo "Visual golden PNG changes require the reviewed-diff file from scripts/regenerate-visual-goldens.sh:" >&2
echo "$all_changed" >&2
echo >&2
echo "Run scripts/regenerate-visual-goldens.sh, inspect the image diff, and commit $REVIEW with the PNG changes." >&2
exit 1
