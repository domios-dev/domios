#!/usr/bin/env bash
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT
source "$ROOT/scripts/dev-notify.sh"
capture_alert() {
  cat "$2" >> "$ALERTS"
  printf '\nEND ALERT\n' >> "$ALERTS"
  if [ -n "${RELEASE_FIFO:-}" ] && rg -q "^EARLY FAILURE" "$2"; then printf 'continue\n' > "$RELEASE_FIFO"; fi
}
export -f capture_alert
export REPORT_NOTIFY_FUNCTION=capture_alert
export QA_REPORT_DIR="$TEST_DIR/reports" QA_NOTIFY_TAB=fixture
export ALERTS="$TEST_DIR/alerts"
: > "$ALERTS"
export RELEASE_FIFO="$TEST_DIR/release"
mkfifo "$RELEASE_FIFO"
# The command cannot finish until the injected sender acknowledges its early alert.
bash "$ROOT/scripts/qa-report.sh" midway 'printf "FAIL [ case one\n"; read -r reply < "$RELEASE_FIFO"; printf "FAIL [ case two\ncontinued\n"; exit 1' > "$TEST_DIR/output" || rc=$?
[ "${rc:-0}" = 1 ]
unset RELEASE_FIFO
[ "$(awk '/^EARLY FAILURE/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
[ "$(awk '/^QA midway: FAIL/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
rg -q 'First failure: FAIL \[ case one' "$ALERTS"
rg -q 'continued' "$TEST_DIR"/reports/midway-*/command-1.log
for fixture in compile gateway timeout node vitest; do
  : > "$ALERTS"
  case "$fixture" in
    compile) command='printf "error[E0308]: mismatch\nerror: could not compile app\n"; exit 1' ;;
    gateway) command='exit 75' ;;
    timeout) command='exit 124' ;;
    node) command='printf "not ok 1 - case\n"; exit 1' ;;
    vitest) command='printf "Tests  1 failed | 2 passed (3)\n"; exit 1' ;;
  esac
  rc=0
  bash "$ROOT/scripts/qa-report.sh" "$fixture" "$command" > "$TEST_DIR/output" || rc=$?
  [ "$rc" = 1 ]
  [ "$(awk '/^EARLY FAILURE/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
  [ "$(awk '/^QA .*: FAIL/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
  case "$fixture" in
    gateway) rg -q 'gateway refused \(exit 75\)' "$ALERTS" ;;
    timeout) rg -q 'timeout cap \(exit 124\)' "$ALERTS" ;;
  esac
done
: > "$ALERTS"
bash "$ROOT/scripts/qa-report.sh" clean 'printf "Tests  3 passed (3)\n"' > "$TEST_DIR/output"
[ "$(awk '/^EARLY FAILURE/ {n++} END {print n+0}' "$ALERTS")" = 0 ]
rg -q '^QA clean: PASS' "$ALERTS"
# The same runner owns dev startup compilation; an alert releases the running build.
: > "$ALERTS"
export RELEASE_FIFO="$TEST_DIR/release"
rc=0
report_run_stream "$TEST_DIR/dev-marker" fixture dev startup "$TEST_DIR/dev.log" \
  bash -c 'printf "error[E0308]: dev mismatch\n"; read -r reply < "$RELEASE_FIFO"; printf "build continued\n"; exit 1' > "$TEST_DIR/output" || rc=$?
unset RELEASE_FIFO
[ "$rc" = 1 ]
[ "$(awk '/^EARLY FAILURE/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
rg -q 'label=dev step=startup' "$ALERTS"
rg -q 'build continued' "$TEST_DIR/dev.log"
# An exit before readiness must produce one failure even for a zero exit.
: > "$ALERTS"
DEV_NOTIFY_TAB=fixture
DEV_NOTIFY_SENT_MARKER="$TEST_DIR/dev-exited"
dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" fixture exited named 7690 "$TEST_DIR/dev.log" 0
dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" fixture exited named 7690 "$TEST_DIR/dev.log" 0
[ "$(awk '/^FAILED stage=exited rc=0/ {n++} END {print n+0}' "$ALERTS")" = 1 ]
# Foreground readiness consumes the same pipe and suppresses later startup alerts.
: > "$ALERTS"
dev_notify_health_proof() {
  dev_notify_ready_once "$TEST_DIR/dev-ready" "$1" named 7690 abc123 4321 "$ROOT"
}
printf 'health event\nerror[E0308]: after READY\n' | report_read_stream "$TEST_DIR/dev-ready" fixture dev launch "$TEST_DIR/dev.log" dev > "$TEST_DIR/output"
[ "$(awk '/^READY / {n++} END {print n+0}' "$ALERTS")" = 1 ]
[ "$(awk '/^EARLY FAILURE/ {n++} END {print n+0}' "$ALERTS")" = 0 ]
printf 'PASS: 7 QA and 3 dev stream fixtures\n'  
