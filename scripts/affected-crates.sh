#!/usr/bin/env bash
# Compute the cargo package selection to test based on what changed vs a base
# ref. Prints either "--workspace" (test everything) or a "-p A -p B ..." list.
#
# The workspace split lets us test only the crates a change touches plus their
# reverse-dependents, instead of recompiling + retesting the whole tree. The app
# crate `codeterm` depends on every domain crate, so any leaf change pulls the
# app in; a leaf change never pulls in a *sibling* leaf (they don't depend on
# each other). Universal leaves (types/util/paths) and infra changes fan out to
# the whole workspace.
#
# Fails SAFE: anything it can't classify → "--workspace".
#
# Usage: scripts/affected-crates.sh [base-ref]   (default origin/master)

set -euo pipefail

BASE="${1:-origin/master}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Merge-base diff when possible (three-dot), else two-dot; empty → test all.
changed="$(git diff --name-only "$BASE"...HEAD 2>/dev/null || true)"
if [ -z "$changed" ]; then
    changed="$(git diff --name-only "$BASE" HEAD 2>/dev/null || true)"
fi
[ -z "$changed" ] && { echo "--workspace"; exit 0; }

# Universal / infra triggers → everything must rebuild + retest.
if printf '%s\n' "$changed" | grep -qE \
    '^(src-tauri/crates/codeterm-(types|util|paths)/|src-tauri/Cargo\.(toml|lock)|Cargo\.(toml|lock)|\.github/workflows/|scripts/|src-tauri/build\.rs)'; then
    echo "--workspace"; exit 0
fi

declare -A pkgs

# App crate: any src-tauri source outside crates/ (src/, tests/, top-level *.rs).
if printf '%s\n' "$changed" | grep -qE '^src-tauri/(src/|tests/|[A-Za-z0-9_]+\.rs$)'; then
    pkgs[codeterm]=1
fi

# Domain crates: a change under one pulls in the crate + the app (reverse dep).
for c in codeterm-mesh codeterm-mem codeterm-plan codeterm-bridge codeterm-pty; do
    if printf '%s\n' "$changed" | grep -qE "^src-tauri/crates/$c/"; then
        pkgs[$c]=1
        pkgs[codeterm]=1
    fi
done

# CLI crate.
if printf '%s\n' "$changed" | grep -qE '^cli/'; then
    pkgs[codeterm-cli]=1
fi

# Nothing recognized (docs-only slips through paths-ignore, but be safe).
[ ${#pkgs[@]} -eq 0 ] && { echo "--workspace"; exit 0; }

out=""
for p in "${!pkgs[@]}"; do
    out+=" -p $p"
done
echo "${out# }"
