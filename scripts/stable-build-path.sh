#!/usr/bin/env bash
# Caller: build.sh

ct_stable_mapping_root() {
  local mappings
  mappings="$(MSYS2_ARG_CONV_EXCL='*' subst)" || return 75
  printf '%s\n' "$mappings" | sed -n "s/^$1:.* => //p" | tr -d '\r' | tr '\\' '/' | tr 'A-Z' 'a-z'
}

ct_stable_mapping_matches() {
  [ "$(ct_stable_mapping_root "$1")" = "$(cygpath -am "$2" | tr 'A-Z' 'a-z')" ]
}

ct_stable_lease_matches() {
  local lease="$1"
  [ "$(cat "$lease/job_id" 2>/dev/null)" = "$CT_BUILD_JOB_ID" ] &&
    [ "$(cat "$lease/pid" 2>/dev/null)" = "${CT_GATEWAY_OWNER_PID:-$$}" ] &&
    [ "$(cat "$lease/start_token" 2>/dev/null)" = "$CT_GATEWAY_PROCESS_START" ] &&
    [ "$(cat "$lease/physical_root" 2>/dev/null)" = "$CT_REAL_WORKTREE" ]
}

ct_stable_mapping_locked() {
  local operation="$1" drive="$2" state="$3" lease="$3/lease" mapped
  ct_gateway_owned_by_caller || return 75
  mapped="$(ct_stable_mapping_root "$drive")" || return $?
  if [ "$operation" = release ]; then
    if ! ct_stable_lease_matches "$lease" || ! ct_stable_mapping_matches "$drive" "$CT_REAL_WORKTREE"; then
      echo "ERROR: stable mapping release refused: mapping or lease identity changed" >&2
      return 75
    fi
  elif [ -d "$lease" ]; then
    if [ ! -s "$lease/job_id" ] || [ ! -s "$lease/pid" ] ||
      [ ! -s "$lease/start_token" ] || [ ! -s "$lease/physical_root" ]; then
      echo "ERROR: stable mapping refused: incomplete lease at $lease" >&2
      return 75
    fi
    if ct_stable_lease_matches "$lease"; then
      ct_stable_mapping_matches "$drive" "$CT_REAL_WORKTREE" || return 75
      return 0
    fi
    if ! ct_gateway_recorded_owner_gone "$lease"; then
      echo "ERROR: stable mapping refused: foreign live or unknown lease at $lease" >&2
      return 75
    fi
    if [ -n "$mapped" ] && ! ct_stable_mapping_matches "$drive" "$(cat "$lease/physical_root")"; then
      echo "ERROR: stable mapping refused: mapping differs from expired lease" >&2
      return 75
    fi
  elif [ -n "$mapped" ]; then
    echo "ERROR: stable mapping refused: existing $drive: mapping has no verifiable lease" >&2
    return 75
  fi
  if [ -n "$mapped" ]; then
    MSYS2_ARG_CONV_EXCL='*' subst "$drive:" /D || return 75
  fi
  if [ -d "$lease" ]; then
    rm -f -- "$lease/job_id" "$lease/physical_root" "$lease/pid" "$lease/start_token" \
      "$lease/pid_namespace" "$lease/native_pid" "$lease/native_start_token"
    rmdir "$lease" || return 75
  fi
  [ "$operation" != release ] || return 0
  mkdir "$lease" || return 75
  ct_gateway_write_owner "$lease" || return 75
  printf '%s\n' "$CT_BUILD_JOB_ID" > "$lease/job_id"
  printf '%s\n' "$CT_REAL_WORKTREE" > "$lease/physical_root"
  MSYS2_ARG_CONV_EXCL='*' subst "$drive:" "$(cygpath -w "$CT_REAL_WORKTREE")" || return 75
  ct_stable_mapping_matches "$drive" "$CT_REAL_WORKTREE" || return 75
}

ct_stable_mapping() {
  local state="$(ct_gateway_dir)/stable-mapping-$2" result=0
  mkdir -p "$state" || return 75
  ct_gateway_mutex_acquire "$state" || return $?
  ct_stable_mapping_locked "$1" "$2" "$state" || result=$?
  ct_gateway_mutex_release "$state" || result=$?
  return "$result"
}
