#!/usr/bin/env bash
# WHAT: Pins chat-roundtrip's terminal machine verdict with and without log capture.
# WHY: A logger pipeline previously turned printed assertion failures into exit zero.
# ORACLE: One forced failure exits 1 and zero failures exit 0 through both paths.
# The verdict-only seam returns before provider, daemon, or filesystem setup.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
RUN="$ROOT/evals/chat-roundtrip/run.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

assert_status() {
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" -ne "$expected" ]; then
    echo "FAIL $label: exit=$actual want=$expected" >&2
    exit 1
  fi
  echo "PASS $label: exit=$actual"
}

assert_summary() {
  local label="$1" log="$2" expected="$3"
  if ! grep -Fq "$expected" "$log"; then
    echo "FAIL $label: missing summary: $expected" >&2
    exit 1
  fi
}

run_direct() {
  local failures="$1" expected_status="$2" expected_summary="$3"
  local log="$TMP_DIR/direct-$failures.log" status
  bash "$RUN" --verdict-only "$failures" >"$log" 2>&1
  status=$?
  assert_status "direct failures=$failures" "$status" "$expected_status"
  assert_summary "direct failures=$failures" "$log" "$expected_summary"
}

run_canonical_capture() {
  local failures="$1" expected_status="$2" expected_summary="$3"
  local log="$TMP_DIR/captured-$failures.log" status
  bash -o pipefail -c 'bash "$1" --verdict-only "$2" 2>&1 | tee "$3"' \
    _ "$RUN" "$failures" "$log"
  status=$?
  assert_status "captured failures=$failures" "$status" "$expected_status"
  assert_summary "captured failures=$failures" "$log" "$expected_summary"
}

run_direct 1 1 "[messaging-core] 1 assertion(s) FAILED"
run_direct 0 0 "[messaging-core] ALL PASS"
run_canonical_capture 1 1 "[messaging-core] 1 assertion(s) FAILED"
run_canonical_capture 0 0 "[messaging-core] ALL PASS"
