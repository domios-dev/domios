#!/usr/bin/env bash

ct_gateway_dir() {
  local home local_app_data
  if [ -n "${CODETERM_BUILD_GATEWAY_DIR:-}" ]; then
    echo "ERROR: CODETERM_BUILD_GATEWAY_DIR is not a supported production override" >&2
    return 2
  fi
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = "fixture" ]; then
    [ -n "${CT_BUILD_GATEWAY_FIXTURE_DIR:-}" ] || { echo "ERROR: fixture gateway directory is empty" >&2; return 2; }
    printf '%s\n' "$CT_BUILD_GATEWAY_FIXTURE_DIR"
  elif [[ "$OSTYPE" == darwin* ]]; then
    home="$(dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
    printf '%s\n' "${home:-$HOME}/Library/Application Support/CodeTerm/build-gateway"
  elif [[ "$OSTYPE" == linux* ]]; then
    home="$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6)"
    printf '%s\n' "${home:-$HOME}/.local/state/codeterm-build-gateway"
  else
    local_app_data="$(powershell.exe -NoProfile -Command "[Console]::Write([Environment]::GetFolderPath('LocalApplicationData'))" 2>/dev/null | tr -d '\r')"
    if [ -z "$local_app_data" ] && [ -n "${LOCALAPPDATA:-}" ]; then
      local_app_data="$LOCALAPPDATA"
    fi
    if [ -z "$local_app_data" ] && [ -n "${USERPROFILE:-}" ]; then
      local_app_data="$USERPROFILE/AppData/Local"
    fi
    if [ -z "$local_app_data" ] && [ -n "${ProgramData:-}" ]; then
      local_app_data="$ProgramData"
    fi
    [ -n "$local_app_data" ] || { echo "ERROR: cannot resolve canonical LocalApplicationData" >&2; return 2; }
    printf '%s\n' "$local_app_data/CodeTerm/build-gateway"
  fi
}

ct_gateway_set_fixture_dir() {
  [ -n "$1" ] || { echo "ERROR: gateway fixture directory is empty" >&2; return 2; }
  CT_BUILD_GATEWAY_SCOPE=fixture
  CT_BUILD_GATEWAY_FIXTURE_DIR="$1"
  export CT_BUILD_GATEWAY_SCOPE CT_BUILD_GATEWAY_FIXTURE_DIR
}

ct_gateway_now() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

ct_gateway_state_tool() {
  node "$(ct_repo_root)/scripts/build-gateway-state.mjs" "$@"
}

ct_gateway_process_start_token() {
  local pid="$1" value rest
  [ -n "$pid" ] || return 1
  if [ -r "/proc/$pid/stat" ]; then
    value="$(cat "/proc/$pid/stat" 2>/dev/null)" || return 1
    rest="${value#*) }"
    awk '{print $20}' <<< "$rest"
    return
  fi
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]] && command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command \
      "\$p=Get-Process -Id $pid -ErrorAction SilentlyContinue; if (\$p) { [Console]::Write(\$p.StartTime.ToUniversalTime().Ticks) } else { exit 1 }" \
      2>/dev/null | tr -d '\r'
    return ${PIPESTATUS[0]}
  fi
  ps -p "$pid" -o lstart= 2>/dev/null | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

ct_gateway_identity_alive() {
  local pid="$1" expected="$2" actual
  [ -n "$pid" ] && [ -n "$expected" ] || return 1
  actual="$(ct_gateway_process_start_token "$pid" 2>/dev/null || true)"
  [ -n "$actual" ] && [ "$actual" = "$expected" ]
}

ct_gateway_process_command() {
  local pid="$1"
  if [ -r "/proc/$pid/cmdline" ]; then
    tr '\0' ' ' < "/proc/$pid/cmdline"
    return
  fi
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]] && command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command \
      "\$p=Get-CimInstance Win32_Process -Filter 'ProcessId=$pid' -ErrorAction SilentlyContinue; if (\$p) { [Console]::Write(\$p.CommandLine) } else { exit 1 }" \
      2>/dev/null | tr -d '\r'
    return ${PIPESTATUS[0]}
  fi
  ps -p "$pid" -o command= 2>/dev/null
}

ct_gateway_terminate_descendant_tree() {
  local pid="$1" child
  kill -STOP "$pid" 2>/dev/null || true
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    ct_gateway_terminate_descendant_tree "$child"
  done
  kill -KILL "$pid" 2>/dev/null || true
}

ct_gateway_terminate_descendants() {
  local root_pid="${BASHPID:-$$}" child descendants
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    descendants="$(ps -W | awk -v root="$root_pid" '
      NR > 1 { parent[$1] = $2; count++ }
      END {
        depth[root] = 0
        for (pass = 0; pass <= count; pass++) {
          for (pid in parent) {
            if (!(pid in depth) && (parent[pid] in depth)) {
              depth[pid] = depth[parent[pid]] + 1
              if (depth[pid] > maximum) maximum = depth[pid]
            }
          }
        }
        for (level = 1; level <= maximum; level++) {
          for (pid in depth) {
            if (depth[pid] == level) print pid
          }
        }
      }
    ')"
    for child in $descendants; do
      kill -KILL "$child" 2>/dev/null || true
    done
    for child in $descendants; do
      wait "$child" 2>/dev/null || true
    done
    return
  fi
  for child in $(pgrep -P "$root_pid" 2>/dev/null || true); do
    ct_gateway_terminate_descendant_tree "$child"
    wait "$child" 2>/dev/null || true
  done
}

ct_gateway_mutex_timeout() {
  CT_GATEWAY_ABORT_DESCENDANTS=1
  export CT_GATEWAY_ABORT_DESCENDANTS
  echo "Timed out acquiring build gateway state mutex." >&2
  echo "Run './build.sh gateway-status' to inspect ownership, then './build.sh gateway-cancel <job-id>' for a stale job." >&2
  ct_gateway_terminate_descendants
  return 75
}

ct_gateway_mutex_acquire() {
  local dir="$1" mutex="$1/state.lock" deadline stale_pid stale_start tombstone self_pid incomplete_seen=0
  self_pid="${CT_GATEWAY_MUTEX_PID:-$$}"
  deadline=$(( $(date +%s) + 30 ))
  while ! mkdir "$mutex" 2>/dev/null; do
    stale_pid="$(cat "$mutex/pid" 2>/dev/null || true)"
    stale_start="$(cat "$mutex/start_token" 2>/dev/null || true)"
    if [ -z "$stale_pid" ] || [ -z "$stale_start" ]; then
      incomplete_seen=$((incomplete_seen + 1))
      if [ "$incomplete_seen" -lt 20 ]; then
        sleep 0.05
        continue
      fi
    else
      incomplete_seen=0
    fi
    if ! ct_gateway_identity_alive "$stale_pid" "$stale_start"; then
      tombstone="$dir/state.lock.stale.$self_pid.$RANDOM"
      if mv "$mutex" "$tombstone" 2>/dev/null; then
        rm -rf "$tombstone"
        continue
      fi
    fi
    [ "$(date +%s)" -le "$deadline" ] || { ct_gateway_mutex_timeout; return $?; }
    sleep 0.05
  done
  printf '%s\n' "$self_pid" > "$mutex/pid"
  CT_GATEWAY_MUTEX_START="$(ct_gateway_process_start_token "$self_pid")"
  [ -n "$CT_GATEWAY_MUTEX_START" ] || { rm -rf "$mutex"; return 2; }
  printf '%s\n' "$CT_GATEWAY_MUTEX_START" > "$mutex/start_token"
}

ct_gateway_mutex_release() {
  local mutex="$1/state.lock" self_pid="${CT_GATEWAY_MUTEX_PID:-$$}"
  if [ "$(cat "$mutex/pid" 2>/dev/null || true)" = "$self_pid" ] &&
      [ "$(cat "$mutex/start_token" 2>/dev/null || true)" = "${CT_GATEWAY_MUTEX_START:-}" ]; then
    rm -rf "$mutex"
  fi
}

ct_gateway_status() {
  local dir rc
  dir="$(ct_gateway_dir)"
  mkdir -p "$dir/jobs" "$dir/queue"
  ct_gateway_mutex_acquire "$dir" || return $?
  ct_gateway_reap_stale_owner_locked "$dir"
  ct_gateway_reap_stale_queue_locked "$dir"
  set +e
  ct_gateway_state_tool status --dir "$dir" --limit "${CODETERM_BUILD_GATEWAY_HISTORY:-20}"
  rc=$?
  set -e
  ct_gateway_mutex_release "$dir"
  return "$rc"
}

ct_gateway_first_ticket() {
  local dir="$1" ticket
  for ticket in "$dir"/queue/*; do
    [ -d "$ticket" ] && basename "$ticket"
  done | sort | head -1
}

ct_gateway_update_job() {
  local state="$1" exit_code="$2" outcome="$3" started_at="${4:-}" ended_at="${5:-}"
  local args=(update --dir "$(ct_gateway_dir)" --job-id "$CT_BUILD_JOB_ID" --state "$state")
  [ -n "$exit_code" ] && args+=(--exit-code "$exit_code")
  [ -n "$outcome" ] && args+=(--outcome "$outcome")
  [ -n "$started_at" ] && args+=(--started-at "$started_at")
  [ -n "$ended_at" ] && args+=(--ended-at "$ended_at")
  ct_gateway_state_tool "${args[@]}"
}

ct_gateway_reap_stale_owner_locked() {
  local dir="$1" lock="$dir/lock" pid start_token job
  [ -d "$lock" ] || return 0
  pid="$(cat "$lock/pid" 2>/dev/null || true)"
  start_token="$(cat "$lock/start_token" 2>/dev/null || true)"
  if ! ct_gateway_identity_alive "$pid" "$start_token"; then
    job="$(cat "$lock/job_id" 2>/dev/null || true)"
    if [ -n "$job" ] && [ -f "$dir/jobs/$job.json" ]; then
      CT_BUILD_JOB_ID="$job" ct_gateway_update_job failed 255 owner_lost "" "$(ct_gateway_now)" || true
    fi
    rm -rf "$lock"
  fi
}

ct_gateway_reap_stale_queue_locked() {
  local dir="$1" ticket pid start_token job
  for ticket in "$dir"/queue/*; do
    [ -d "$ticket" ] || continue
    pid="$(cat "$ticket/pid" 2>/dev/null || true)"
    start_token="$(cat "$ticket/start_token" 2>/dev/null || true)"
    ct_gateway_identity_alive "$pid" "$start_token" && continue
    job="$(cat "$ticket/job_id" 2>/dev/null || true)"
    if [ -n "$job" ] && [ -f "$dir/jobs/$job.json" ]; then
      CT_BUILD_JOB_ID="$job" ct_gateway_update_job failed 255 owner_lost "" "$(ct_gateway_now)" || true
    fi
    rm -rf "$ticket"
  done
}

ct_gateway_finish() {
  local rc="$1" dir lock state outcome lock_pid lock_start
  [ "${CT_GATEWAY_ACTIVE:-0}" = "1" ] || return 0
  CT_GATEWAY_ACTIVE=0
  dir="$(ct_gateway_dir)"
  lock="$dir/lock"
  ct_gateway_mutex_acquire "$dir" || return $?
  rm -rf "$dir/queue/${CT_BUILD_QUEUE_KEY:-missing}"
  if [ "${CT_GATEWAY_CANCELLED:-0}" = "1" ] || [ "$rc" -eq 130 ] || [ "$rc" -eq 143 ]; then
    state=cancelled
    outcome=cancelled
  elif [ "$rc" -eq 0 ]; then
    state=completed
    outcome=success
  else
    state=failed
    outcome=command_failed
  fi
  ct_gateway_update_job "$state" "$rc" "$outcome" "" "$(ct_gateway_now)" || true
  lock_pid="$(cat "$lock/pid" 2>/dev/null || true)"
  lock_start="$(cat "$lock/start_token" 2>/dev/null || true)"
  if [ -d "$lock" ] && [ "$(cat "$lock/job_id" 2>/dev/null || true)" = "$CT_BUILD_JOB_ID" ] &&
      ct_gateway_identity_alive "$lock_pid" "$lock_start"; then
    rm -rf "$lock"
  fi
  ct_gateway_mutex_release "$dir"
}

ct_gateway_on_exit() {
  local rc="$?"
  trap - EXIT INT TERM
  if [ "${CT_GATEWAY_ABORT_DESCENDANTS:-0}" = "1" ]; then
    ct_gateway_terminate_descendants
    exit "$rc"
  fi
  ct_gateway_finish "$rc"
  exit "$rc"
}

ct_gateway_on_signal() {
  CT_GATEWAY_CANCELLED=1
  exit 130
}

ct_gateway_acquire() {
  local command="$1" dir lock wait_mode deadline first owner job log queued_at source_sha worktree owner_command
  dir="$(ct_gateway_dir)"
  lock="$dir/lock"
  wait_mode="${CODETERM_BUILD_GATEWAY_MODE:-queue}"
  deadline=$(( $(date +%s) + ${CODETERM_BUILD_GATEWAY_WAIT_SECS:-1800} ))
  mkdir -p "$dir/jobs" "$dir/queue"
  CT_GATEWAY_PROCESS_START="$(ct_gateway_process_start_token "$$")"
  [ -n "$CT_GATEWAY_PROCESS_START" ] || { echo "Cannot determine gateway owner process start identity" >&2; return 2; }
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = "fixture" ]; then
    CT_BUILD_JOB_ID="${CT_BUILD_JOB_ID:-$(date -u +%Y%m%dT%H%M%S)-$$-${RANDOM:-0}}"
  else
    CT_BUILD_JOB_ID="$(date -u +%Y%m%dT%H%M%S)-$$-${RANDOM:-0}"
  fi
  CT_BUILD_QUEUE_KEY="$(node -p 'Date.now()')-$(printf '%010d' "$$")-$CT_BUILD_JOB_ID"
  log="$dir/jobs/$CT_BUILD_JOB_ID.log"
  if [ -e "$dir/jobs/$CT_BUILD_JOB_ID.json" ]; then
    echo "Build gateway job id already exists: $CT_BUILD_JOB_ID" >&2
    return 2
  fi
  owner="${CODETERM_BUILD_OWNER_ID:-${CODETERM_TAB_ID:-pid-$$}}"
  job="${command%% *}"
  queued_at="$(ct_gateway_now)"
  source_sha="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  worktree="$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)"
  owner_command="$(ct_gateway_process_command "$$" 2>/dev/null || true)"
  export CT_BUILD_JOB_ID CT_BUILD_QUEUE_KEY CT_GATEWAY_PROCESS_START
  trap 'ct_gateway_on_exit' EXIT
  trap 'ct_gateway_on_signal' INT TERM
  ct_gateway_mutex_acquire "$dir" || return $?
  ct_gateway_reap_stale_owner_locked "$dir"
  ct_gateway_reap_stale_queue_locked "$dir"
  if [ -e "$dir/jobs/$CT_BUILD_JOB_ID.json" ]; then
    ct_gateway_mutex_release "$dir"
    echo "Build gateway job id already exists: $CT_BUILD_JOB_ID" >&2
    return 2
  fi
  : > "$log"
  mkdir "$dir/queue/$CT_BUILD_QUEUE_KEY"
  printf '%s\n' "$$" > "$dir/queue/$CT_BUILD_QUEUE_KEY/pid"
  printf '%s\n' "$CT_GATEWAY_PROCESS_START" > "$dir/queue/$CT_BUILD_QUEUE_KEY/start_token"
  printf '%s\n' "$CT_BUILD_JOB_ID" > "$dir/queue/$CT_BUILD_QUEUE_KEY/job_id"
  local recorded_worktree="$worktree"
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = "fixture" ] && [ -n "${CODETERM_BUILD_WORKTREE_ID:-}" ]; then
    recorded_worktree="$CODETERM_BUILD_WORKTREE_ID"
  fi
  ct_gateway_state_tool create --dir "$dir" --job-id "$CT_BUILD_JOB_ID" --job "$job" \
    --command "$command" --owner "$owner" --owner-pid "$$" --owner-start-token "$CT_GATEWAY_PROCESS_START" \
    --owner-process-command "$owner_command" \
    --worktree "$recorded_worktree" \
    --source-sha "$source_sha" --queue-key "$CT_BUILD_QUEUE_KEY" --queued-at "$queued_at" \
    --log-path "$log"
  ct_gateway_mutex_release "$dir"
  CT_GATEWAY_ACTIVE=1

  while :; do
    if [ ! -d "$dir/queue/$CT_BUILD_QUEUE_KEY" ]; then
      CT_GATEWAY_CANCELLED=1
      return 130
    fi
    ct_gateway_mutex_acquire "$dir" || return $?
    ct_gateway_reap_stale_owner_locked "$dir"
    ct_gateway_reap_stale_queue_locked "$dir"
    first="$(ct_gateway_first_ticket "$dir")"
    if [ "$first" = "$CT_BUILD_QUEUE_KEY" ] && mkdir "$lock" 2>/dev/null; then
      printf '%s\n' "$$" > "$lock/pid"
      printf '%s\n' "$CT_GATEWAY_PROCESS_START" > "$lock/start_token"
      printf '%s\n' "$CT_BUILD_JOB_ID" > "$lock/job_id"
      printf '%s\n' "$CT_BUILD_QUEUE_KEY" > "$lock/queue_key"
      printf '%s\n' "$command" > "$lock/command"
      printf '%s\n' "$worktree" > "$lock/worktree"
      printf '%s\n' "$source_sha" > "$lock/source_sha"
      ct_gateway_update_job running "" "" "$(ct_gateway_now)" ""
      rm -rf "$dir/queue/$CT_BUILD_QUEUE_KEY"
      ct_gateway_mutex_release "$dir"
      return 0
    fi
    ct_gateway_mutex_release "$dir"
    if [ "$wait_mode" = "fail-fast" ]; then
      return 75
    fi
    if [ "$(date +%s)" -gt "$deadline" ]; then
      return 75
    fi
    sleep "${CODETERM_BUILD_GATEWAY_POLL_SECS:-0.2}"
  done
}

ct_gateway_cancel() {
  local job_id="$1" dir record state queue_key pid start_token rc=0
  dir="$(ct_gateway_dir)"
  record="$dir/jobs/$job_id.json"
  ct_gateway_mutex_acquire "$dir" || return $?
  ct_gateway_reap_stale_owner_locked "$dir"
  ct_gateway_reap_stale_queue_locked "$dir"
  if [ ! -f "$record" ]; then
    ct_gateway_mutex_release "$dir"
    echo "Unknown build gateway job: $job_id" >&2
    return 2
  fi
  state="$(node -e "const j=require(process.argv[1]); process.stdout.write(j.state)" "$record")"
  queue_key="$(node -e "const j=require(process.argv[1]); process.stdout.write(j.queue_key)" "$record")"
  if [ "$state" = "queued" ]; then
    rm -rf "$dir/queue/$queue_key"
    CT_BUILD_JOB_ID="$job_id" ct_gateway_update_job cancelled 130 cancelled "" "$(ct_gateway_now)"
    ct_gateway_mutex_release "$dir"
    return 0
  fi
  if [ "$state" = "running" ] && [ "$(cat "$dir/lock/job_id" 2>/dev/null || true)" = "$job_id" ]; then
    pid="$(cat "$dir/lock/pid" 2>/dev/null || true)"
    start_token="$(cat "$dir/lock/start_token" 2>/dev/null || true)"
    if ! ct_gateway_identity_alive "$pid" "$start_token"; then
      ct_gateway_mutex_release "$dir"
      return 1
    fi
    kill -TERM "$pid" 2>/dev/null || rc=1
    ct_gateway_mutex_release "$dir"
    [ "$rc" -eq 0 ] || return "$rc"
    return 0
  fi
  ct_gateway_mutex_release "$dir"
  echo "Build gateway job $job_id is already $state" >&2
  return 2
}

ct_gateway_assert_internal_child() {
  local job_id="$1" parent_pid="$2" parent_start="$3" expected_command="$4"
  local dir lock record record_command record_worktree record_sha current_sha current_worktree parent_command
  dir="$(ct_gateway_dir)"
  lock="$dir/lock"
  record="$dir/jobs/$job_id.json"
  [ "$PPID" = "$parent_pid" ] || { echo "[build-gateway] internal child parent mismatch" >&2; return 2; }
  ct_gateway_identity_alive "$parent_pid" "$parent_start" || { echo "[build-gateway] internal child parent identity is stale" >&2; return 2; }
  parent_command="$(ct_gateway_process_command "$parent_pid" 2>/dev/null || true)"
  case "$parent_command" in
    *build.sh*) ;;
    *) echo "[build-gateway] internal child parent is not build.sh" >&2; return 2 ;;
  esac
  ct_gateway_mutex_acquire "$dir" || return $?
  if [ "$(cat "$lock/job_id" 2>/dev/null || true)" != "$job_id" ] ||
      [ "$(cat "$lock/pid" 2>/dev/null || true)" != "$parent_pid" ] ||
      [ "$(cat "$lock/start_token" 2>/dev/null || true)" != "$parent_start" ]; then
    ct_gateway_mutex_release "$dir"
    echo "[build-gateway] internal child does not own the live gateway lock" >&2
    return 2
  fi
  record_command="$(node -e "const j=require(process.argv[1]);process.stdout.write(j.command)" "$record" 2>/dev/null || true)"
  record_worktree="$(node -e "const j=require(process.argv[1]);process.stdout.write(j.worktree)" "$record" 2>/dev/null || true)"
  record_sha="$(node -e "const j=require(process.argv[1]);process.stdout.write(j.source_sha)" "$record" 2>/dev/null || true)"
  current_worktree="$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)"
  current_sha="$(git rev-parse HEAD 2>/dev/null || true)"
  if [ "$record_command" != "$expected_command" ] || [ "$record_worktree" != "$current_worktree" ] ||
      [ "$record_sha" != "$current_sha" ] || [ "$(cat "$lock/command")" != "$expected_command" ] ||
      [ "$(cat "$lock/worktree")" != "$current_worktree" ] || [ "$(cat "$lock/source_sha")" != "$current_sha" ]; then
    ct_gateway_mutex_release "$dir"
    echo "[build-gateway] internal child command/worktree/source provenance mismatch" >&2
    return 2
  fi
  ct_gateway_mutex_release "$dir"
}

ct_gateway_log_path() {
  echo "$(ct_gateway_dir)/jobs/${CT_BUILD_JOB_ID:-manual-$$}.log"
}
