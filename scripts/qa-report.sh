#!/usr/bin/env bash

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
source "$SCRIPT_DIR/dev-notify.sh"
dev_notify_capture_parent

usage() {
  echo "Usage: scripts/qa-report.sh <label> '<cmd>' ['<cmd>' ...]" >&2
}

if [ "$#" -lt 2 ]; then
  usage
  exit 2
fi

QA_LABEL="$1"
shift
case "$QA_LABEL" in
  ""|*[!A-Za-z0-9._-]*)
    echo "ERROR: label may contain only letters, numbers, dot, underscore, and dash" >&2
    exit 2
    ;;
esac

QA_BASE="${QA_REPORT_DIR:-/tmp/codeterm-qa}"
QA_STAMP="$(date +%Y%m%d-%H%M%S)"
QA_RUN_DIR="$QA_BASE/$QA_LABEL-$QA_STAMP-$$"
if ! mkdir -p "$QA_BASE" || ! mkdir "$QA_RUN_DIR"; then
  echo "ERROR: could not create a fresh QA report directory: $QA_RUN_DIR" >&2
  exit 2
fi

QA_FULL_LOG="$QA_RUN_DIR/full.log"
QA_SUMMARY="$QA_RUN_DIR/summary.txt"
: > "$QA_FULL_LOG"
: > "$QA_SUMMARY"
QA_TOTAL="$#"
QA_OK=0
QA_INDEX=0
QA_ZERO_TESTS=0

qa_summary_line() {
  printf '%s\n' "$1" >> "$QA_SUMMARY"
  printf '%s\n' "$1"
}

qa_append_matches() {
  local file="$1" mode="$2" extracted="$QA_RUN_DIR/.extract"
  case "$mode" in
    cargo)
      awk '/test result:/ { print }' "$file" > "$extracted"
      ;;
    vitest)
      awk 'tolower($0) ~ /tests .*passed|tests .*failed|no test files found/ { print }' "$file" > "$extracted"
      ;;
    node)
      awk '/^# (tests|pass|fail)[[:space:]]/ { print }' "$file" > "$extracted"
      ;;
  esac
  cat "$extracted" >> "$QA_SUMMARY"
  cat "$extracted"
}

qa_zero_tests() {
  local file="$1"
  if awk 'BEGIN { all_zero=1 } /test result:/ { seen=1; if ($0 !~ /0 passed; 0 failed/) all_zero=0 } END { exit !(seen && all_zero) }' "$file"; then
    return 0
  fi
  if awk 'tolower($0) ~ /no test files found/ || tolower($0) ~ /(^|[^0-9])0 tests?([^0-9]|$)/ { found=1 } END { exit !found }' "$file"; then
    return 0
  fi
  awk '/^# tests[[:space:]]+0[[:space:]]*$/ { found=1 } END { exit !found }' "$file"
}

qa_summary_line "QA report directory: $QA_RUN_DIR"

while [ "$#" -gt 0 ]; do
  QA_INDEX=$((QA_INDEX + 1))
  QA_COMMAND="$1"
  shift
  QA_COMMAND_LOG="$QA_RUN_DIR/command-$QA_INDEX.log"
  printf '===== [%s/%s] %s =====\n' "$QA_INDEX" "$QA_TOTAL" "$QA_COMMAND" >> "$QA_FULL_LOG"
  if bash -o pipefail -c "$QA_COMMAND" > "$QA_COMMAND_LOG" 2>&1; then
    QA_RC=0
  else
    QA_RC=$?
  fi
  cat "$QA_COMMAND_LOG" >> "$QA_FULL_LOG"
  printf '\n' >> "$QA_FULL_LOG"
  qa_summary_line "[$QA_INDEX/$QA_TOTAL] rc=$QA_RC $QA_COMMAND"
  qa_append_matches "$QA_COMMAND_LOG" cargo
  qa_append_matches "$QA_COMMAND_LOG" vitest
  qa_append_matches "$QA_COMMAND_LOG" node
  if qa_zero_tests "$QA_COMMAND_LOG"; then
    QA_ZERO_TESTS=1
    qa_summary_line "ZERO TESTS RAN"
  fi
  awk '(/FAILED|panicked|^error/) && count < 20 { print; count++ }' "$QA_COMMAND_LOG" > "$QA_RUN_DIR/.errors"
  cat "$QA_RUN_DIR/.errors" >> "$QA_SUMMARY"
  cat "$QA_RUN_DIR/.errors"
  if [ "$QA_RC" -ne 0 ]; then
    qa_summary_line "Last 25 output lines for command $QA_INDEX:"
    tail -n 25 "$QA_COMMAND_LOG" >> "$QA_SUMMARY"
    tail -n 25 "$QA_COMMAND_LOG"
  fi
  if [ "$QA_RC" -eq 0 ] && ! qa_zero_tests "$QA_COMMAND_LOG"; then
    QA_OK=$((QA_OK + 1))
  fi
done

if [ "$QA_OK" -eq "$QA_TOTAL" ] && [ "$QA_ZERO_TESTS" -eq 0 ]; then
  QA_VERDICT=PASS
  QA_STATUS=0
else
  QA_VERDICT=FAIL
  QA_STATUS=1
fi
qa_summary_line "QA $QA_LABEL: $QA_VERDICT ($QA_OK/$QA_TOTAL ok)"

if [ -n "${QA_NOTIFY_TAB:-}" ]; then
  dev_notify_send "$QA_NOTIFY_TAB" "$QA_SUMMARY" >/dev/null 2>&1 || QA_STATUS=1
fi

exit "$QA_STATUS"
