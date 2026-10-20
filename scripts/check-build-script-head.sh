#!/usr/bin/env bash
# Caller: build.sh script-guards
# A crate below a root package that tracks git HEAD rebuilds every dependent on each commit; only root packages may.
set -euo pipefail
ROOT="${1:-$(git rev-parse --show-toplevel)}"
cd "$ROOT"

ROOT_PACKAGES='^(src-tauri|cli)/build\.rs$'
GIT_TRACKING='git-path|logs/HEAD|\.git/|Command::new\("git"\)|rustc-env=BUILD_(SHA|HASH)'
failed=0
while IFS= read -r file; do
  [[ "$file" =~ $ROOT_PACKAGES ]] && continue
  if grep -nE "$GIT_TRACKING" "$file" >&2; then
    echo "ERROR: $file tracks git HEAD; embed the commit in the root package and install it through codeterm-build-info" >&2
    failed=1
  fi
done < <(git ls-files -- '*/build.rs' 'build.rs')
[ "$failed" = 0 ] && echo "build-script HEAD guard: only root packages track git HEAD"
exit "$failed"
