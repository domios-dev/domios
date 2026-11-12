#!/usr/bin/env bash

CT_BUILD_GATEWAY_LOGGING_FAILURE=74

wait_build_gateway_reader() {
  local pid="$1" deadline="$2" token watchdog status=0
  token="$(ct_gateway_process_start_token "$pid" 2>/dev/null || true)"
  node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-reader-deadline.mjs" "$pid" "$token" "$deadline" </dev/null >&3 2>&4 &
  watchdog=$!
  wait "$pid" || status=$?
  kill "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true
  return "$status"
}

finish_build_gateway_logging() {
  local reader_status=0 trace_status="${CT_BUILD_GATEWAY_TRACE_DEGRADED:-0}" deadline=10000
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = fixture ]; then
    deadline="${CT_BUILD_GATEWAY_READER_TIMEOUT_MS:-10000}"
  fi
  exec 2>&4
  if [ -n "${CT_BUILD_GATEWAY_TRACE_READER_PID:-}" ]; then
    : > "$CT_BUILD_GATEWAY_TRACE_DONE" || return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
    wait_build_gateway_reader "$CT_BUILD_GATEWAY_TRACE_READER_PID" "$deadline" || trace_status=$?
    node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-output.mjs" fingerprint-summary "$CT_BUILD_GATEWAY_TRACE_PATH"
  fi
  exec 1>&3
  : > "$CT_BUILD_GATEWAY_LOG_DONE" || return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  if [ -n "${CT_BUILD_GATEWAY_LOG_READER_STATUS:-}" ]; then
    reader_status="$CT_BUILD_GATEWAY_LOG_READER_STATUS"
  else
    wait_build_gateway_reader "$CT_BUILD_GATEWAY_LOG_READER_PID" "$deadline" || reader_status=$?
  fi
  rm -f -- "$CT_BUILD_GATEWAY_LOG_DONE"
  [ -z "${CT_BUILD_GATEWAY_TRACE_DONE:-}" ] || rm -f -- "$CT_BUILD_GATEWAY_TRACE_DONE"
  exec 3>&- 4>&-
  [ "$trace_status" = 0 ] || return "$trace_status"
  return "$reader_status"
}

complete_build_gateway_before_handoff() {
  local reader_status=0 finish_status=0 outcome=""
  trap - EXIT INT TERM
  if [ -n "${CT_BUILD_GATEWAY_LOG_PATH:-}" ]; then
    printf 'Build gateway job %s build exit: 0\n' "$CT_BUILD_JOB_ID"
    finish_build_gateway_logging || reader_status=$?
    if [ "$reader_status" -ne 0 ]; then
      printf 'ERROR: build gateway logging path broke before launch handoff (reader exit %s); evidence is incomplete; job failed; log: %s\n' \
        "$reader_status" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
      outcome=logging_failed
      finish_status="$CT_BUILD_GATEWAY_LOGGING_FAILURE"
    fi
  fi
  ct_gateway_finish "$finish_status" 0 "$outcome" || finish_status=$?
  return "$finish_status"
}

build_gateway_logging_on_exit() {
  local captured_status="$?" command_status="" command_status_text=UNKNOWN
  local summary_status reader_status finish_status
  trap - EXIT INT TERM
  set +e
  release_stable_build_path
  if [ "${CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN:-0}" = "1" ] && [ "${CT_GATEWAY_CANCELLED:-0}" != "1" ]; then
    command_status="$captured_status"
    command_status_text="$captured_status"
  fi
  printf 'Build gateway job %s exit: %s\n' "$CT_BUILD_JOB_ID" "$command_status_text"
  summary_status=$?
  finish_build_gateway_logging
  reader_status=$?
  if [ "$summary_status" -ne 0 ] || [ "$reader_status" -ne 0 ]; then
    printf 'ERROR: build gateway logging path broke (exit %s); command exit was %s; log: %s\n' \
      "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "$command_status_text" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
    ct_gateway_finish "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "$command_status" logging_failed
    finish_status=$?
    [ "$finish_status" -eq 0 ] || exit "$finish_status"
    exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  ct_gateway_finish "$captured_status" "$command_status"
  finish_status=$?
  [ "$finish_status" -eq 0 ] || exit "$finish_status"
  exit "$captured_status"
}

run_build_gateway_log_reader() {
  local stuck=0
  if [ "${CT_BUILD_GATEWAY_FAIL_LOG_OPEN:-0}" = "1" ]; then
    printf 'ERROR: injected build gateway log open failure: %s\n' "$1" >&2
    return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = fixture ]; then stuck="${CT_BUILD_GATEWAY_STUCK_READER:-0}"; fi
  exec node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-log-reader.mjs" "$1" "$CT_BUILD_GATEWAY_LOG_DONE" "$stuck"
}

enter_build_gateway() {
  ct_gateway_acquire "$*" || return $?
  local gateway_log
  CT_BUILD_ARTIFACT_DIR="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.artifacts"
  mkdir -p "$CT_BUILD_ARTIFACT_DIR"
  export CT_BUILD_ARTIFACT_DIR
  gateway_log="$(ct_gateway_log_path)"
  echo "Build gateway job $CT_BUILD_JOB_ID log: $gateway_log"
  [ "$CMD" = check-queue ] || [ "$CMD" = check-workspace ] || enter_stable_build_path || return $?
  # Runs after the stable mapping so retention sees X: and refreshes the recorded gateway build directory.
  if [ "$CMD" != gc ]; then
    ct_cache_retention auto || echo "[cache-policy] retention refused; build continues" >&2
  fi
  if [ "${CT_BUILD_GATEWAY_NO_TEE:-0}" = "1" ] && [ "${CODETERM_BUILD_FINGERPRINTS:-0}" != 1 ]; then
    echo "WARNING: build gateway log duplication is disabled by CT_BUILD_GATEWAY_NO_TEE=1" >&2
    return
  fi
  exec 3>&1 4>&2
  trap '' PIPE
  unset CT_BUILD_GATEWAY_LOG_READER_STATUS
  CT_BUILD_GATEWAY_LOG_DONE="${gateway_log}.done.$$"
  run_build_gateway_log_reader "$gateway_log" </dev/null >&3 2>&4 &
  CT_BUILD_GATEWAY_LOG_READER_PID=$!
  exec >> "$gateway_log" 2>&1
  CT_BUILD_GATEWAY_LOG_PATH="$gateway_log"
  CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN=0
  trap 'build_gateway_logging_on_exit' EXIT
  if [ "${CODETERM_BUILD_FINGERPRINTS:-0}" = 1 ]; then
    CT_BUILD_GATEWAY_TRACE_PATH="${gateway_log%.log}.fingerprint.log"
    CT_BUILD_GATEWAY_TRACE_INPUT="${gateway_log%.log}.stderr.log"
    CT_BUILD_GATEWAY_TRACE_DONE="${gateway_log}.trace.done.$$"
    : > "$CT_BUILD_GATEWAY_TRACE_INPUT" || exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
    node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-log-reader.mjs" "$CT_BUILD_GATEWAY_TRACE_INPUT" "$CT_BUILD_GATEWAY_TRACE_DONE" 0 "$CT_BUILD_GATEWAY_TRACE_PATH" </dev/null 2>&4 &
    CT_BUILD_GATEWAY_TRACE_READER_PID=$!
    exec 2>> "$CT_BUILD_GATEWAY_TRACE_INPUT"
  fi
  if [ "${CT_BUILD_GATEWAY_FAIL_LOG_OPEN:-0}" = "1" ]; then
    if wait "$CT_BUILD_GATEWAY_LOG_READER_PID"; then
      CT_BUILD_GATEWAY_LOG_READER_STATUS=0
    else
      CT_BUILD_GATEWAY_LOG_READER_STATUS=$?
    fi
    exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN=1
}
