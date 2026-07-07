#!/usr/bin/env bash
# CI test runner for the src-tauri workspace.
#
# Integration branches (master/develop/feat/*/release-*) run the FULL workspace —
# comprehensive gate. Lane/PR builds run only what a change touches:
#
#   - app source changed  → TEST the app crate (+ any changed leaf crates)
#   - ONLY leaf crate(s)   → TEST those leaves, and only cargo-CHECK the app
#                            (compile it so a broken API is caught, but skip the
#                            app's ~1700 tests — that run is the time sink, not
#                            the compile)
#   - types/util/paths or infra → full workspace
#
# Fails SAFE: any diff it can't classify → full workspace. It never SKIPS tests
# for changed code — worst case it runs more. The integration gate is the backstop.
#
# Env: CI_TEST_BASE (default origin/master) — the ref to diff against.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MANIFEST="--manifest-path src-tauri/Cargo.toml"
LEAVES="codeterm-mesh codeterm-mem codeterm-plan codeterm-bridge codeterm-pty"

full() { echo "[ci-test] $1 → full workspace"; exec cargo test --workspace $MANIFEST --lib; }

REF="${GITHUB_REF_NAME:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)}"
BASE="${CI_TEST_BASE:-origin/master}"

case "$REF" in
    master|develop|feat/*|release-*) full "$REF (integration)" ;;
esac

# Ensure the diff base exists (checkout may not track origin/master).
if [ "$BASE" = "origin/master" ]; then
    git fetch --quiet origin master:refs/remotes/origin/master 2>/dev/null || true
fi

changed="$(git diff --name-only "$BASE"...HEAD 2>/dev/null || true)"
[ -z "$changed" ] && changed="$(git diff --name-only "$BASE" HEAD 2>/dev/null || true)"
[ -z "$changed" ] && full "$REF (no diff)"

# Universal / infra → everything.
if printf '%s\n' "$changed" | grep -qE \
    '^(src-tauri/crates/codeterm-(types|util|paths)/|src-tauri/Cargo\.(toml|lock)|Cargo\.(toml|lock)|\.github/workflows/|scripts/|src-tauri/build\.rs)'; then
    full "$REF (universal/infra change)"
fi

test_pkgs=""
app_changed=0
# App crate: any src-tauri source outside crates/.
if printf '%s\n' "$changed" | grep -qE '^src-tauri/(src/|tests/|[A-Za-z0-9_]+\.rs$)'; then
    app_changed=1
    test_pkgs+=" -p codeterm"
fi
leaf_changed=0
for c in $LEAVES; do
    if printf '%s\n' "$changed" | grep -qE "^src-tauri/crates/$c/"; then
        test_pkgs+=" -p $c"
        leaf_changed=1
    fi
done
# CLI is its own manifest; only test it when cli/ changed (handled by the caller's
# separate "CLI tests" step). Here we only touch the src-tauri workspace.

# Nothing in the src-tauri workspace changed (e.g. cli-only or docs) → skip.
if [ -z "$test_pkgs" ]; then
    echo "[ci-test] $REF → no src-tauri workspace changes, skipping lib tests"
    exit 0
fi

test_pkgs="${test_pkgs# }"
echo "[ci-test] $REF → test:$test_pkgs"
cargo test $test_pkgs $MANIFEST --lib

# Leaf changed but app source didn't: compile the app against the new leaf so an
# API break is caught, but skip the app's expensive test run.
if [ "$leaf_changed" = 1 ] && [ "$app_changed" = 0 ]; then
    echo "[ci-test] $REF → leaf-only: cargo check -p codeterm (skip app test run)"
    cargo check -p codeterm $MANIFEST
fi
