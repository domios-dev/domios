#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="${TMPDIR:-/tmp}/codeterm-release-signing-$$"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT"

if CODETERM_TAURI_SIGNING_KEY_FILE="$TEST_ROOT/missing.key" bash -c \
    "source '$ROOT/scripts/release-signing.sh'; ct_load_release_signing_key" \
    > "$TEST_ROOT/missing.log" 2>&1; then
  echo "missing signing key unexpectedly passed" >&2
  exit 1
fi
grep -q "refusing to compile an unsigned updater" "$TEST_ROOT/missing.log"

printf 'fixture-secret\r\n' > "$TEST_ROOT/key"
value="$(CODETERM_TAURI_SIGNING_KEY_FILE="$TEST_ROOT/key" bash -c \
  "source '$ROOT/scripts/release-signing.sh'; ct_load_release_signing_key; printf '%s' \"\$TAURI_SIGNING_PRIVATE_KEY\"")"
[ "$value" = "fixture-secret" ]

value="$(TAURI_SIGNING_PRIVATE_KEY=from-env CODETERM_TAURI_SIGNING_KEY_FILE="$TEST_ROOT/missing.key" bash -c \
  "source '$ROOT/scripts/release-signing.sh'; ct_load_release_signing_key; printf '%s' \"\$TAURI_SIGNING_PRIVATE_KEY\"")"
[ "$value" = "from-env" ]

echo "PASS release signing loads local key, preserves env override, and fails early when missing"
