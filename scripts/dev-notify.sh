#!/usr/bin/env bash
# Caller: build.sh helpers; scripts qa-report.sh, tests/qa-report-early-alerts.sh, tests/utility-shell-reports.test.mjs

dev_notify_capture_parent() {
  DEV_NOTIFY_PARENT_CLI="$(command -v codeterm 2>/dev/null || true)"
  DEV_NOTIFY_PARENT_CODETERM_API_URL_SET="${CODETERM_API_URL+x}"
  DEV_NOTIFY_PARENT_CODETERM_API_URL="${CODETERM_API_URL-}"
  DEV_NOTIFY_PARENT_CODETERM_API_TOKEN_SET="${CODETERM_API_TOKEN+x}"
  DEV_NOTIFY_PARENT_CODETERM_API_TOKEN="${CODETERM_API_TOKEN-}"
  DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH_SET="${CODETERM_SOCKET_PATH+x}"
  DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH="${CODETERM_SOCKET_PATH-}"
  DEV_NOTIFY_PARENT_CODETERM_DATA_DIR_SET="${CODETERM_DATA_DIR+x}"
  DEV_NOTIFY_PARENT_CODETERM_DATA_DIR="${CODETERM_DATA_DIR-}"
  DEV_NOTIFY_PARENT_CODETERM_TAB_ID_SET="${CODETERM_TAB_ID+x}"
  DEV_NOTIFY_PARENT_CODETERM_TAB_ID="${CODETERM_TAB_ID-}"
  DEV_NOTIFY_PARENT_CODETERM_SESSION_ID_SET="${CODETERM_SESSION_ID+x}"
  DEV_NOTIFY_PARENT_CODETERM_SESSION_ID="${CODETERM_SESSION_ID-}"
  DEV_NOTIFY_PARENT_CODETERM_SESSION_BINDING_NONCE_SET="${CODETERM_SESSION_BINDING_NONCE+x}"
  DEV_NOTIFY_PARENT_CODETERM_SESSION_BINDING_NONCE="${CODETERM_SESSION_BINDING_NONCE-}"
  DEV_NOTIFY_PARENT_CODETERM_WORKER_ID_SET="${CODETERM_WORKER_ID+x}"
  DEV_NOTIFY_PARENT_CODETERM_WORKER_ID="${CODETERM_WORKER_ID-}"
  DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID_SET="${CODETERM_PARENT_TAB_ID+x}"
  DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID="${CODETERM_PARENT_TAB_ID-}"
  DEV_NOTIFY_PARENT_CODETERM_TEAM_ID_SET="${CODETERM_TEAM_ID+x}"
  DEV_NOTIFY_PARENT_CODETERM_TEAM_ID="${CODETERM_TEAM_ID-}"
}

dev_notify_parent_cli() {
  if [ -z "${DEV_NOTIFY_PARENT_CLI:-}" ]; then
    echo "ERROR: parent codeterm CLI was not available when notifications were captured" >&2
    return 127
  fi
  (
    local name
    for name in $(compgen -A variable CODETERM_); do
      unset "$name" 2>/dev/null || true
    done
    if [ "${DEV_NOTIFY_PARENT_CODETERM_API_URL_SET:-}" = x ]; then export CODETERM_API_URL="$DEV_NOTIFY_PARENT_CODETERM_API_URL"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_API_TOKEN_SET:-}" = x ]; then export CODETERM_API_TOKEN="$DEV_NOTIFY_PARENT_CODETERM_API_TOKEN"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH_SET:-}" = x ]; then export CODETERM_SOCKET_PATH="$DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_DATA_DIR_SET:-}" = x ]; then export CODETERM_DATA_DIR="$DEV_NOTIFY_PARENT_CODETERM_DATA_DIR"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_TAB_ID_SET:-}" = x ]; then export CODETERM_TAB_ID="$DEV_NOTIFY_PARENT_CODETERM_TAB_ID"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_SESSION_ID_SET:-}" = x ]; then export CODETERM_SESSION_ID="$DEV_NOTIFY_PARENT_CODETERM_SESSION_ID"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_SESSION_BINDING_NONCE_SET:-}" = x ]; then export CODETERM_SESSION_BINDING_NONCE="$DEV_NOTIFY_PARENT_CODETERM_SESSION_BINDING_NONCE"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_WORKER_ID_SET:-}" = x ]; then export CODETERM_WORKER_ID="$DEV_NOTIFY_PARENT_CODETERM_WORKER_ID"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID_SET:-}" = x ]; then export CODETERM_PARENT_TAB_ID="$DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_TEAM_ID_SET:-}" = x ]; then export CODETERM_TEAM_ID="$DEV_NOTIFY_PARENT_CODETERM_TEAM_ID"; fi
    "$DEV_NOTIFY_PARENT_CLI" "$@"
  )
}

dev_notify_resolve_target() {
  local explicit_tab="${1:-}" shell_tab="${DEV_NOTIFY_PARENT_CODETERM_TAB_ID:-}"
  local parent_hint="${DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID:-}" tab_model="" team_id="" team_model="" resolved=""
  DEV_NOTIFY_TARGET_KIND=none
  DEV_NOTIFY_TARGET_MESH=""
  DEV_NOTIFY_TAB=""
  if [ -n "$shell_tab" ] && [ -n "${DEV_NOTIFY_PARENT_CLI:-}" ]; then
    tab_model="$(dev_notify_parent_cli tab inspect --json --tab "$shell_tab" 2>/dev/null || true)"
    if [ -z "$explicit_tab" ]; then
      team_id="$(printf '%s' "$tab_model" | node scripts/dev-notify-target.mjs --team-id "$shell_tab" 2>/dev/null || true)"
      if [ -n "$team_id" ]; then
        team_model="$(dev_notify_parent_cli team show "$team_id" --json 2>/dev/null || true)"
      fi
    fi
  fi
  resolved="$(printf '%s' "$tab_model" | node scripts/dev-notify-target.mjs "$explicit_tab" "$shell_tab" "$parent_hint" "$team_model" 2>/dev/null || true)"
  IFS=$'\t' read -r DEV_NOTIFY_TARGET_KIND DEV_NOTIFY_TAB DEV_NOTIFY_TARGET_MESH <<< "$resolved"
  [ -n "$DEV_NOTIFY_TARGET_KIND" ] || DEV_NOTIFY_TARGET_KIND=none
}

dev_notify_send() {
  local tab_id="$1" body_file="$2" level="${3:-failure}" title="${4:-Build failed}" first_line=""
  if [ "$#" -lt 3 ] && [ -f "$body_file" ]; then
    IFS= read -r first_line < "$body_file" || true
    case "$first_line" in
      OK*|READY*) level=verbose; title="Build complete" ;;
    esac
  fi
  if [ -n "${REPORT_NOTIFY_FUNCTION:-}" ]; then
    "$REPORT_NOTIFY_FUNCTION" "$tab_id" "$body_file" "${DEV_NOTIFY_TARGET_MESH:-}" "$level" "$title"
    return $?
  fi
  if [ ! -f "$body_file" ]; then
    echo "ERROR: notification body file does not exist: $body_file" >&2
    return 2
  fi
  if [ "${DEV_NOTIFY_TARGET_KIND:-send}" = notify ]; then
    local pad_dir pad_file pad_name severity status
    pad_dir="$(dev_notify_parent_cli pad path)" || return $?
    case "$pad_dir" in
      [A-Za-z]:\\*) pad_dir="$(cygpath -u "$pad_dir")" || return $? ;;
    esac
    pad_file="$(mktemp "$pad_dir/build-report.XXXXXX")" || return $?
    if ! cp "$body_file" "$pad_file"; then rm -f "$pad_file"; return 1; fi
    pad_name="${pad_file##*/}"
    if [ "$level" = failure ]; then severity=error; else severity=info; fi
    if dev_notify_parent_cli notify send "$title" --from-pad "$pad_name" --level "$level" --severity "$severity"; then
      rm -f "$pad_file"
      return 0
    else
      status=$?
      rm -f "$pad_file"
      return "$status"
    fi
  fi
  local target_args=(--tab "$tab_id")
  [ -z "${DEV_NOTIFY_TARGET_MESH:-}" ] || target_args=(--mesh "$DEV_NOTIFY_TARGET_MESH" --tab "$tab_id")
  local provenance_args=()
  if [ -z "${DEV_NOTIFY_PARENT_CODETERM_TAB_ID:-}" ]; then
    provenance_args=(--system-context build)
  fi
  dev_notify_parent_cli send "${target_args[@]}" --from-file "$body_file" "${provenance_args[@]}" >/dev/null
}

dev_notify_write_ready() {
  local body_file="$1" instance="$2" api_port="$3" build_sha="$4" listener_pid="$5" checkout="$6"
  if [ -z "$instance" ]; then instance=default; fi
  printf 'READY instance=%s api_port=%s build=%s\n' "$instance" "$api_port" "$build_sha" > "$body_file"
}

dev_notify_write_failed() {
  local body_file="$1" stage="$2" instance="$3" api_port="$4" log_file="$5" rc="${6:-}"
  local compiler_errors="" first_error=""
  if [ -z "$instance" ]; then instance=default; fi
  if [ -n "$rc" ]; then
    printf 'FAILED stage=%s rc=%s instance=%s api_port=%s\n' "$stage" "$rc" "$instance" "$api_port" > "$body_file"
  else
    printf 'FAILED stage=%s instance=%s api_port=%s\n' "$stage" "$instance" "$api_port" > "$body_file"
  fi
  case "$rc" in
    75) printf 'Reason: gateway refused (exit 75)\n' >> "$body_file" ;;
    124) printf 'Reason: timeout cap (exit 124)\n' >> "$body_file" ;;
  esac
  printf 'Log: %s\n' "$log_file" >> "$body_file"
  if [ -f "$log_file" ]; then
    compiler_errors="$(grep -E 'error(\[E[0-9]+\])?:|fatal error|LNK[0-9]{4}|undefined reference' "$log_file" 2>/dev/null | head -n 5 || true)"
    if [ -n "$compiler_errors" ]; then
      printf 'Compiler errors:\n%s\n' "$compiler_errors" >> "$body_file"
    else
      first_error="$(grep -m 1 -E '^(ERROR:|error:|Diff in |.*could not compile)' "$log_file" 2>/dev/null || true)"
      if [ -n "$first_error" ]; then printf 'First failure: %s\n' "$first_error" >> "$body_file"; fi
    fi
  fi
}

dev_notify_send_once() {
  local marker="$1" tab_id="$2" body_file="$3" level="${4:-failure}" title="${5:-Build failed}" status first_line=""
  if [ "$#" -lt 4 ] && [ -f "$body_file" ]; then
    IFS= read -r first_line < "$body_file" || true
    case "$first_line" in
      OK*|READY*) level=verbose; title="Build complete" ;;
    esac
  fi
  DEV_NOTIFY_SEND_CLAIMED=0
  if ! mkdir "$marker" 2>/dev/null; then
    rm -f "$body_file"
    return 0
  fi
  DEV_NOTIFY_SEND_CLAIMED=1
  dev_notify_send "$tab_id" "$body_file" "$level" "$title"
  status=$?
  rm -f "$body_file"
  return "$status"
}

dev_notify_new_body_file() {
  mktemp "${TMPDIR:-/tmp}/codeterm-dev-notify.XXXXXX"
}

dev_notify_ready_once() {
  local marker="$1" tab_id="$2" instance="$3" api_port="$4" build_sha="$5" listener_pid="$6" checkout="$7"
  local body_file
  DEV_NOTIFY_SEND_CLAIMED=0
  body_file="$(dev_notify_new_body_file)" || return 1
  dev_notify_write_ready "$body_file" "$instance" "$api_port" "$build_sha" "$listener_pid" "$checkout" || return 1
  dev_notify_send_once "$marker" "$tab_id" "$body_file" verbose "Dev ready"
}

# The notice only reports readiness the caller already proved, so its failure never fails DEV.
dev_notify_ready_after_proof() {
  if ! dev_notify_ready_once "$@"; then
    echo "WARNING: DEV is ready, but its readiness notice to tab $2 failed; DEV keeps running." >&2
  fi
}

dev_notify_failure_once() {
  local marker="$1" tab_id="$2" stage="$3" instance="$4" api_port="$5" log_file="$6" rc="${7:-}"
  local body_file
  DEV_NOTIFY_SEND_CLAIMED=0
  body_file="$(dev_notify_new_body_file)" || return 1
  dev_notify_write_failed "$body_file" "$stage" "$instance" "$api_port" "$log_file" "$rc" || return 1
  dev_notify_send_once "$marker" "$tab_id" "$body_file"
}

dev_notify_is_sent() {
  [ -d "$1" ]
}

dev_notify_waiter_cleanup() {
  local fifo="$1" tail_pid="$2"
  if [ -n "$tail_pid" ]; then
    kill "$tail_pid" 2>/dev/null || true
    wait "$tail_pid" 2>/dev/null || true
  fi
  if [ "${DEV_NOTIFY_WAITER_FD_OPEN:-0}" = 1 ]; then
    exec 3<&-
    DEV_NOTIFY_WAITER_FD_OPEN=0
  fi
  rm -f "$fifo"
}

dev_notify_wait_stream() {
  local log_file="$1" deadline_s="$2" launcher_pid="$3" tab_id="$4"
  local fifo tail_pid deadline remaining line read_status result=0 stream_token stream_status
  if [ ! -f "$log_file" ]; then : > "$log_file" || return 1; fi
  fifo="${TMPDIR:-/tmp}/codeterm-dev-notify.$$.${RANDOM}.fifo"
  stream_token="__DEV_NOTIFY_TAIL_$$-${RANDOM}__"
  if ! mkfifo "$fifo"; then
    dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" stream "${CT_INSTANCE:-}" "${CT_PORT:-7686}" "$log_file" 1
    return 1
  fi
  deadline=$(( $(date +%s) + deadline_s ))
  (
    stream_tail_pid=
    dev_notify_tail_wrapper_cleanup() {
      if [ -n "$stream_tail_pid" ]; then
        kill "$stream_tail_pid" 2>/dev/null || true
        wait "$stream_tail_pid" 2>/dev/null || true
      fi
      exit 143
    }
    trap 'dev_notify_tail_wrapper_cleanup' INT TERM
    printf '%s:START\n' "$stream_token"
    tail -F -n +1 "$log_file" &
    stream_tail_pid=$!
    if [ -n "${DEV_NOTIFY_TAIL_PID_FILE:-}" ]; then printf '%s\n' "$stream_tail_pid" > "$DEV_NOTIFY_TAIL_PID_FILE"; fi
    stream_status=0
    wait "$stream_tail_pid" || stream_status=$?
    stream_tail_pid=
    trap - INT TERM
    printf '\n%s:EXIT:%s\n' "$stream_token" "$stream_status"
  ) > "$fifo" 2>/dev/null &
  tail_pid=$!
  trap 'dev_notify_waiter_cleanup "$fifo" "$tail_pid"' EXIT
  trap 'dev_notify_waiter_cleanup "$fifo" "$tail_pid"; exit 130' INT
  trap 'dev_notify_waiter_cleanup "$fifo" "$tail_pid"; exit 143' TERM
  exec 3< "$fifo"
  DEV_NOTIFY_WAITER_FD_OPEN=1
  if [ "${DEV_NOTIFY_TEST_MODE:-0}" = 1 ]; then printf 'DEV_NOTIFY_WAITER_STARTED launcher_pid=%s tail_pid=%s\n' "$launcher_pid" "$tail_pid"; fi
  while :; do
    remaining=$(( deadline - $(date +%s) ))
    if [ "$remaining" -le 0 ]; then
      dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" deadline "${CT_INSTANCE:-}" "${CT_PORT:-7686}" "$log_file"
      result=$?
      break
    fi
    line=
    IFS= read -r -t "$remaining" line <&3
    read_status=$?
    if [ "$read_status" -ne 0 ]; then
      # bash 3.2 (macOS /bin/bash) reports a read -t timeout as 1, not >128 like bash 4+.
      if [ "$read_status" -gt 128 ] || [ "$(date +%s)" -ge "$deadline" ]; then
        dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" deadline "${CT_INSTANCE:-}" "${CT_PORT:-7686}" "$log_file"
      else
        dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" stream "${CT_INSTANCE:-}" "${CT_PORT:-7686}" "$log_file" "$read_status"
      fi
      result=$?
      break
    fi
    if [ "$line" = "${stream_token}:START" ]; then continue; fi
    case "$line" in
      "${stream_token}:EXIT:"*)
        stream_status="${line#"${stream_token}:EXIT:"}"
        dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" stream "${CT_INSTANCE:-}" "${CT_PORT:-7686}" "$log_file" "$stream_status"
        result=$?
        break
        ;;
    esac
    if dev_notify_is_sent "$DEV_NOTIFY_SENT_MARKER"; then break; fi
    if type dev_notify_health_proof >/dev/null 2>&1 && dev_notify_health_proof "$tab_id"; then
      result=0
      break
    fi
  done
  dev_notify_waiter_cleanup "$fifo" "$tail_pid"
  trap - EXIT INT TERM
  return "$result"
}

dev_notify_exit_report() {
  local status="$1" instance="${CT_INSTANCE:-${CODETERM_INSTANCE:-}}" api_port="${CT_PORT:-${CODETERM_API_PORT:-7686}}" sent_by_us="${DEV_NOTIFY_SEND_CLAIMED:-0}"
  local log_file="${DEV_NOTIFY_LOG:-/tmp/codeterm-dev.log}" stage="${DEV_NOTIFY_STAGE:-exit}"
  if [ -z "${CT_PORT+x}" ] && [ "$api_port" = 7685 ] && [ -z "$instance" ]; then api_port=7686; fi
  if [ "$status" -ne 0 ] && [ -n "${DEV_NOTIFY_TAB:-}" ] && ! dev_notify_is_sent "$DEV_NOTIFY_SENT_MARKER"; then
    dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$stage" "$instance" "$api_port" "$log_file" "$status" || true
    sent_by_us="$DEV_NOTIFY_SEND_CLAIMED"
  fi
  if [ -n "${DEV_NOTIFY_WAITER_PID:-}" ]; then
    if [ "$sent_by_us" = 1 ]; then
      dev_notify_stop_stream_waiter "$DEV_NOTIFY_WAITER_PID"
    else
      wait "$DEV_NOTIFY_WAITER_PID" 2>/dev/null || true
    fi
    DEV_NOTIFY_WAITER_PID=
  fi
  if [ -n "${DEV_NOTIFY_SENT_MARKER:-}" ] && [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ]; then rm -rf "$DEV_NOTIFY_SENT_MARKER"; fi
}

dev_notify_stop_stream_waiter() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

REPORT_FAILURE_PATTERN='error\[E[0-9]+\]|^(ERROR|Diff[[:space:]]in)[:[:space:]]|error: could not compile|(^|[[:space:]])FAIL[:[:space:]]|^[[:space:]]*not[[:space:]]ok[[:space:]]|(Tests|Test[[:space:]]Files)[[:space:]].*[1-9][0-9]*[[:space:]]failed|panicked at|test result: FAILED|✗ FAILED'

report_failure_line() {
  local line="$1"
  line="$(printf '%s' "$line" | sed $'s/\033\\[[0-9;]*[mK]//g')"
  [[ "$line" =~ $REPORT_FAILURE_PATTERN ]]
}

# Gateway bookkeeping printed after the failing command is never the cause.
report_first_failure() {
  local log="$1" clean
  clean="$(sed $'s/\033\\[[0-9;]*[mK]//g' "$log" 2>/dev/null)" || return 0
  printf '%s\n' "$clean" | grep -m 1 -E "$REPORT_FAILURE_PATTERN" ||
    printf '%s\n' "$clean" | grep -v -E '^[[:space:]]*$|^\[build-gateway\]' | tail -n 1
}

report_alert_once() {
  local marker="$1" tab="$2" label="$3" step="$4" line="$5" log="$6" body compiler_errors
  [ -n "$tab" ] || return 0
  [ ! -d "$marker" ] || return 0
  body="$(dev_notify_new_body_file)" || return 1
  printf 'EARLY FAILURE label=%s step=%s\nFirst failure: %s\nLog: %s\n' "$label" "$step" "$line" "$log" > "$body"
  compiler_errors="$(grep -E 'error(\[E[0-9]+\])?:|fatal error|LNK[0-9]{4}|undefined reference' "$log" 2>/dev/null | head -n 5 || true)"
  if [ -n "$compiler_errors" ]; then printf 'Compiler errors:\n%s\n' "$compiler_errors" >> "$body"; fi
  dev_notify_send_once "$marker" "$tab" "$body"
}

report_read_stream() {
  local marker="$1" tab="$2" label="$3" step="$4" log="$5" line
  local health="${6:-}" deadline=$((SECONDS + 600)) remaining status
  while :; do
    line=
    status=0
    if [ "$health" = dev ] && ! dev_notify_is_sent "$marker"; then
      remaining=$((deadline - SECONDS))
      if [ "$remaining" -gt 0 ]; then
        IFS= read -r -t "$remaining" line || status=$?
      else
        status=124
      fi
      if [ "$status" -ne 0 ] && [ "$SECONDS" -ge "$deadline" ]; then
        report_alert_once "$marker" "$tab" "$label" "$step" "timeout cap (exit 124)" "$log" || true
        health=
        continue
      fi
    else
      IFS= read -r line || status=$?
    fi
    if [ "$status" -ne 0 ] && [ -z "$line" ]; then break; fi
    printf '%s\n' "$line"
    if report_failure_line "$line"; then
      report_alert_once "$marker" "$tab" "$label" "$step" "$line" "$log" || true
    fi
    if [ "$health" = dev ] && ! dev_notify_is_sent "$marker"; then
      dev_notify_health_proof "$tab" || true
    fi
  done
}

report_run_stream() {
  local marker="$1" tab="$2" label="$3" step="$4" log="$5" rc reason
  local statuses
  shift 5
  if "$@" 2>&1 | tee -a "$log" | report_read_stream "$marker" "$tab" "$label" "$step" "$log"; then
    statuses=("${PIPESTATUS[@]}")
  else
    statuses=("${PIPESTATUS[@]}")
  fi
  rc="${statuses[0]}"
  if [ "$rc" -eq 0 ]; then rc="${statuses[1]}"; fi
  if [ "$rc" -eq 0 ]; then rc="${statuses[2]}"; fi
  case "$rc" in
    0) return 0 ;;
    75) reason="gateway refused (exit 75)" ;;
    124) reason="timeout cap (exit 124)" ;;
    *) reason="step exited non-zero (exit $rc): $(report_first_failure "$log")" ;;
  esac
  report_alert_once "$marker" "$tab" "$label" "$step" "$reason" "$log" || true
  return "$rc"
}
