#!/usr/bin/env bash
# CI test runner for the src-tauri workspace.
#
# Integration branches (master/develop/feat/*/release-*) run the FULL workspace —
# comprehensive gate, and the place we actually cover every crate's tests. Lane
# and PR builds run only the crates a change touches plus their reverse-dependents
# (see affected-crates.sh), for fast worker feedback. The selection fails SAFE:
# if it can't compute a diff (shallow checkout, detached), it falls back to the
# full workspace, so a bad guess never SKIPS tests — it only ever runs more.
#
# Env: CI_TEST_BASE (default origin/master) — the ref to diff against.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REF="${GITHUB_REF_NAME:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)}"
BASE="${CI_TEST_BASE:-origin/master}"

case "$REF" in
    master|develop|feat/*|release-*)
        echo "[ci-test] $REF is an integration branch → full workspace"
        exec cargo test --workspace --manifest-path src-tauri/Cargo.toml --lib
        ;;
esac

# Ensure the diff base exists locally (checkout may not track origin/master).
# Failure is fine — affected-crates.sh falls back to the full workspace.
if [ "$BASE" = "origin/master" ]; then
    git fetch --quiet origin master:refs/remotes/origin/master 2>/dev/null || true
fi

sel="$(bash scripts/affected-crates.sh "$BASE")"
echo "[ci-test] $REF → affected selection: $sel"
# shellcheck disable=SC2086  # sel is an intentional multi-token flag list
exec cargo test $sel --manifest-path src-tauri/Cargo.toml --lib
