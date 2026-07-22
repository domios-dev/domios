#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="${TMPDIR:-/tmp}/codeterm-provenance-test-$$"
ARTIFACT="$TEST_ROOT/artifact.txt"
MANIFEST="$TEST_ROOT/manifest.json"
SOURCE_INPUT="$ROOT/.research/build-perf/provenance-source-$$.txt"
GENERATED_FIXTURE="$ROOT/dist/.provenance-fixture-$$"
mkdir -p "$TEST_ROOT"
mkdir -p "$(dirname "$SOURCE_INPUT")"
trap 'rm -rf "$TEST_ROOT"; rm -f "$SOURCE_INPUT" "$GENERATED_FIXTURE"' EXIT
printf 'fresh artifact\n' > "$ARTIFACT"
printf 'source revision one\n' > "$SOURCE_INPUT"

node scripts/cache-provenance.mjs write \
  --kind fixture --profile test --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain \
  --artifact "$ARTIFACT" --out "$MANIFEST"
node scripts/cache-provenance.mjs check \
  --kind fixture --profile test --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain --manifest "$MANIFEST"

printf 'source revision two\n' > "$SOURCE_INPUT"
if node scripts/cache-provenance.mjs check \
    --kind fixture --profile test --target fixture-target --flags fixture-flags \
    --features fixture-features --toolchain-fixture fixture-toolchain \
    --manifest "$MANIFEST" > "$TEST_ROOT/dirty-content.log" 2>&1; then
  echo "dirty same-path source content unexpectedly reused provenance" >&2
  exit 1
fi
grep -q "source SHA/status mismatch" "$TEST_ROOT/dirty-content.log"
printf 'source revision one\n' > "$SOURCE_INPUT"

printf 'stale artifact\n' > "$ARTIFACT"
if node scripts/cache-provenance.mjs check \
    --kind fixture --profile test --target fixture-target --flags fixture-flags \
    --features fixture-features --toolchain-fixture fixture-toolchain \
    --manifest "$MANIFEST" > "$TEST_ROOT/stale.log" 2>&1; then
  echo "stale artifact provenance unexpectedly passed" >&2
  exit 1
fi
grep -q "artifact hash mismatch" "$TEST_ROOT/stale.log"

SNAPSHOT="$TEST_ROOT/input-snapshot.json"
node scripts/cache-provenance.mjs snapshot \
  --kind fixture-input --profile release --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain --out "$SNAPSHOT"
node scripts/cache-provenance.mjs check \
  --kind fixture-input --profile release --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain --manifest "$SNAPSHOT"
if node scripts/cache-provenance.mjs check \
    --kind fixture-input --profile debug --target fixture-target --flags fixture-flags \
    --features fixture-features --toolchain-fixture fixture-toolchain \
    --manifest "$SNAPSHOT" > "$TEST_ROOT/profile.log" 2>&1; then
  echo "mismatched input provenance unexpectedly passed" >&2
  exit 1
fi
grep -q "profile mismatch" "$TEST_ROOT/profile.log"

FULL_BUILD_INPUT="$TEST_ROOT/full-build-input.json"
node scripts/cache-provenance.mjs snapshot \
  --kind full-build-input --profile release --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain --out "$FULL_BUILD_INPUT"
printf 'generated refresh\n' > "$GENERATED_FIXTURE"
node scripts/cache-provenance.mjs check \
  --kind full-build-input --profile release --target fixture-target --flags fixture-flags \
  --features fixture-features --toolchain-fixture fixture-toolchain --manifest "$FULL_BUILD_INPUT"

echo "PASS cache provenance freshness and artifact hash rejection"
