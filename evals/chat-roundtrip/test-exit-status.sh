#!/usr/bin/env bash
# WHY: A logger pipeline previously turned printed assertion failures into exit zero.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
RUN="$ROOT/evals/chat-roundtrip/run.sh"
CHECKED_RUN="$ROOT/evals/chat-roundtrip/run-checked.ps1"
TMP_DIR="$(mktemp -d)"
PS_LOG_DIR="target-check/evals/chat-roundtrip/exit-status-$$"
trap 'rm -rf "$TMP_DIR" "$ROOT/$PS_LOG_DIR"' EXIT

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

run_powershell_capture() {
  local failures="$1" expected_status="$2" expected_summary="$3"
  local log="$PS_LOG_DIR/verdict-$failures.log" status
  local powershell git_dir no_bash_path
  if command -v pwsh >/dev/null; then
    powershell="$(command -v pwsh)"
  elif command -v pwsh.exe >/dev/null; then
    powershell="$(command -v pwsh.exe)"
  elif command -v powershell.exe >/dev/null; then
    powershell="$(command -v powershell.exe)"
  else
    echo "SKIP PowerShell caller oracle: PowerShell is unavailable"
    return
  fi
  git_dir="$(dirname "$(command -v git)")"
  no_bash_path="$git_dir:/c/Windows/System32"
  if ! PATH="$no_bash_path" command -v git >/dev/null; then
    echo "FAIL PowerShell caller oracle: git is missing from the stripped PATH" >&2
    exit 1
  fi
  if PATH="$no_bash_path" command -v bash >/dev/null; then
    echo "FAIL PowerShell caller oracle: bash remains on the stripped PATH" >&2
    exit 1
  fi
  env PATH="$no_bash_path" "$powershell" -NoProfile -NonInteractive -File "$CHECKED_RUN" \
    -LogPath "$log" -VerdictOnly "$failures"
  status=$?
  assert_status "PowerShell no-PATH-bash failures=$failures" "$status" "$expected_status"
  assert_summary "PowerShell no-PATH-bash failures=$failures" "$ROOT/$log" "$expected_summary"
}

run_direct 1 1 "[messaging-core] 1 assertion(s) FAILED"
run_direct 0 0 "[messaging-core] ALL PASS"
run_canonical_capture 1 1 "[messaging-core] 1 assertion(s) FAILED"
run_canonical_capture 0 0 "[messaging-core] ALL PASS"
run_powershell_capture 1 1 "[messaging-core] 1 assertion(s) FAILED"
run_powershell_capture 0 0 "[messaging-core] ALL PASS"
