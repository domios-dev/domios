#!/usr/bin/env bash
# Completion is explicit: bash 3.2 can report a parse failure to EXIT as status zero.
BENCH_ASSERTIONS=0
BENCH_COMPLETE=0

bench_assertion() { BENCH_ASSERTIONS=$((BENCH_ASSERTIONS + 1)); }
bench_complete() { BENCH_COMPLETE=1; }
bench_status() {
  local status="$1"
  if [ "$status" -ne 0 ]; then return "$status"; fi
  if [ "$BENCH_ASSERTIONS" -eq 0 ] || [ "$BENCH_COMPLETE" -ne 1 ]; then
    echo "FAIL bench incomplete or empty: assertions=$BENCH_ASSERTIONS complete=$BENCH_COMPLETE" >&2
    return 1
  fi
  echo "ASSERTIONS EXECUTED: $BENCH_ASSERTIONS"
}
bench_on_exit() {
  local status=$?
  trap - EXIT
  set +e
  if declare -F cleanup >/dev/null; then cleanup; fi
  bench_status "$status"
  status=$?
  exit "$status"
}
bench_require() {
  local prerequisite
  for prerequisite in "$@"; do
    command -v "$prerequisite" >/dev/null || {
      echo "FAIL missing prerequisite: $prerequisite" >&2
      exit 7
    }
  done
}
trap bench_on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Bash 3.2 cannot wait on process-substitution children; a FIFO retains a waitable PID.
bench_stream_start() {
  local stream_dir
  stream_dir="$(mktemp -d)" || return
  mkfifo "$stream_dir/output" || { rmdir "$stream_dir"; return 7; }
  "$@" >"$stream_dir/output" &
  BENCH_STREAM_PID=$!
  exec 8<"$stream_dir/output"
  rm -rf "$stream_dir"
}
