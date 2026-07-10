#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-perf/health-test-$$"
FIXTURE="$TEST_ROOT/health.json"
mkdir -p "$TEST_ROOT"
cleanup() {
  local rc="$?"
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT
SHA=abc123456789abc123456789abc123456789abcd
printf '%s\n' "{\"status\":\"ok\",\"build\":\"abc1234\",\"build_sha\":\"$SHA\",\"instance\":{\"name\":\"fixture\",\"api_port\":\"7787\",\"vite_port\":\"1500\",\"cdp_port\":\"9363\",\"identifier\":\"com.codeterm.dev.fixture\"}}" > "$FIXTURE"

./build.sh gateway-health-check "$SHA" fixture 7787 1500 9363 com.codeterm.dev.fixture "$FIXTURE" > "$TEST_ROOT/pass.log"
grep -q "^$SHA$" "$TEST_ROOT/pass.log"

assert_rejected() {
  local name="$1" expected="$2"; shift 2
  if ./build.sh gateway-health-check "$@" "$FIXTURE" > "$TEST_ROOT/$name.log" 2>&1; then
    echo "$name identity mismatch unexpectedly passed health validation" >&2
    exit 1
  fi
  grep -q "$expected" "$TEST_ROOT/$name.log"
}

assert_rejected short-sha "stale or wrong-lane" abc123456789 fixture 7787 1500 9363 com.codeterm.dev.fixture
assert_rejected wrong-sha "stale or wrong-lane" fbc123456789abc123456789abc123456789abcd fixture 7787 1500 9363 com.codeterm.dev.fixture
printf '%s\n' "{\"status\":\"stale\",\"build_sha\":\"$SHA\",\"instance\":{\"name\":\"fixture\",\"api_port\":\"7787\",\"vite_port\":\"1500\",\"cdp_port\":\"9363\",\"identifier\":\"com.codeterm.dev.fixture\"}}" > "$FIXTURE"
assert_rejected status "status=ok" "$SHA" fixture 7787 1500 9363 com.codeterm.dev.fixture
printf '%s\n' "{\"status\":\"ok\",\"build_sha\":\"$SHA\",\"instance\":{\"name\":\"fixture\",\"api_port\":\"7787\",\"vite_port\":\"1500\",\"cdp_port\":\"9363\",\"identifier\":\"com.codeterm.dev.fixture\"}}" > "$FIXTURE"
assert_rejected name "wrong instance" "$SHA" other 7787 1500 9363 com.codeterm.dev.fixture
assert_rejected api "wrong instance" "$SHA" fixture 7788 1500 9363 com.codeterm.dev.fixture
assert_rejected vite "wrong instance" "$SHA" fixture 7787 1501 9363 com.codeterm.dev.fixture
assert_rejected cdp "wrong instance" "$SHA" fixture 7787 1500 9364 com.codeterm.dev.fixture
assert_rejected identifier "wrong instance" "$SHA" fixture 7787 1500 9363 com.codeterm.dev.other
printf '%s\n' "{\"status\":\"ok\",\"build_sha\":\"$SHA\",\"instance\":{\"name\":\"\",\"api_port\":\"7686\",\"vite_port\":\"1420\",\"cdp_port\":\"9223\",\"identifier\":\"com.codeterm.dev\"}}" > "$FIXTURE"
./build.sh gateway-health-check "$SHA" "" 7686 1420 9223 com.codeterm.dev "$FIXTURE" > "$TEST_ROOT/empty-name.log"
grep -q "^$SHA$" "$TEST_ROOT/empty-name.log"
echo "PASS health build SHA and instance mismatch validation"
