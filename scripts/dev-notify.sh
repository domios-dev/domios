#!/usr/bin/env bash

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
}

dev_notify_send() {
  local tab_id="$1" body_file="$2" name
  if [ -z "${DEV_NOTIFY_PARENT_CLI:-}" ]; then
    echo "ERROR: parent codeterm CLI was not available when notifications were captured" >&2
    return 127
  fi
  if [ ! -f "$body_file" ]; then
    echo "ERROR: notification body file does not exist: $body_file" >&2
    return 2
  fi
  (
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
    "$DEV_NOTIFY_PARENT_CLI" send --tab "$tab_id" --from-file "$body_file"
  )
}

dev_notify_write_ready() {
  local body_file="$1" instance="$2" api_port="$3" build_sha="$4" listener_pid="$5" checkout="$6"
  if [ -z "$instance" ]; then instance=default; fi
  printf 'READY instance=%s api_port=%s build=%s listener_pid=%s checkout=%s\n' \
    "$instance" "$api_port" "$build_sha" "$listener_pid" "$checkout" > "$body_file"
}

dev_notify_write_failed() {
  local body_file="$1" stage="$2" instance="$3" api_port="$4" log_file="$5" rc="${6:-}"
  if [ -z "$instance" ]; then instance=default; fi
  if [ -n "$rc" ]; then
    printf 'FAILED stage=%s rc=%s instance=%s api_port=%s\n' "$stage" "$rc" "$instance" "$api_port" > "$body_file"
  else
    printf 'FAILED stage=%s instance=%s api_port=%s\n' "$stage" "$instance" "$api_port" > "$body_file"
  fi
  if [ -f "$log_file" ]; then tail -n 25 "$log_file" >> "$body_file" 2>/dev/null || true; fi
}

dev_notify_send_once() {
  local marker="$1" tab_id="$2" body_file="$3" status
  DEV_NOTIFY_SEND_CLAIMED=0
  if ! mkdir "$marker" 2>/dev/null; then
    rm -f "$body_file"
    return 0
  fi
  DEV_NOTIFY_SEND_CLAIMED=1
  dev_notify_send "$tab_id" "$body_file"
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
  dev_notify_send_once "$marker" "$tab_id" "$body_file"
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
      if [ "$read_status" -gt 128 ]; then
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
  if [ -n "${DEV_NOTIFY_SENT_MARKER:-}" ]; then rm -rf "$DEV_NOTIFY_SENT_MARKER"; fi
}

dev_notify_stop_stream_waiter() {
  local pid="$1"
  [ -n "$pid" ] || return 0
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}
