#!/usr/bin/env bash
# Staleness check: AGENTS.md must equal the output of gen-agents-md.sh.
#
# Fails (exit 1) when CLAUDE.md was edited without regenerating AGENTS.md, so
# the two instruction files can never silently diverge. Wire into CI alongside
# the other source-of-truth checks (see B7's docs-source-of-truth.yml).
#
# Usage:
#   scripts/check-agents-md.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GEN="$ROOT/scripts/gen-agents-md.sh"
OUT="$ROOT/AGENTS.md"

if [ ! -f "$OUT" ]; then
    echo "❌ AGENTS.md is missing — run scripts/gen-agents-md.sh and commit it" >&2
    exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
bash "$(dirname "$0")/gen-agents-md.sh" --stdout >"$tmp"

if ! diff -u "$OUT" "$tmp"; then
    echo >&2
    echo "❌ AGENTS.md is stale vs CLAUDE.md — run scripts/gen-agents-md.sh and commit the result" >&2
    exit 1
fi

echo "✅ AGENTS.md is in sync with CLAUDE.md"
