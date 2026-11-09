#!/usr/bin/env bash
# Caller: build.sh lane-check

configure_lane_build_dir() {
  local directory
  directory="$(CODETERM_RUST_CACHE_ROOT="${CODETERM_RUST_CACHE_ROOT:-}" CARGO_HOME="${CARGO_HOME:-}" \
    node "$(dirname "${BASH_SOURCE[0]}")/lane-build-dir.mjs")" || return $?
  export CARGO_BUILD_BUILD_DIR="$directory"
}

configure_lane_cache() {
  if [ "${CODETERM_USE_SCCACHE:-1}" = 0 ]; then
    echo "[lane-check] ERROR: sccache is required; remove CODETERM_USE_SCCACHE=0" >&2
    return 2
  fi
  if ! command -v sccache >/dev/null 2>&1; then
    echo "[lane-check] ERROR: sccache unavailable; install it before running lane-check" >&2
    return 2
  fi
  ct_sccache_with_lock ct_ensure_shared_sccache_locked
}

configure_lane_workspace_wrapper() {
  local directory wrapper cache_wrapper="${RUSTC_WRAPPER:-}"
  directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || return $?
  wrapper="$directory/lane-rustc-wrapper.sh"
  case "${OSTYPE:-}" in msys*|cygwin*) wrapper="$(node "$directory/lane-native-wrapper.mjs")" || return $? ;; esac
  # Cargo hashes the wrapper path for workspace units, while registry units stay shared.
  export RUSTC_WORKSPACE_WRAPPER="$wrapper"
  unset CODETERM_LANE_CACHE_WRAPPER
  if [ -n "$cache_wrapper" ] && [ "$cache_wrapper" != "$wrapper" ]; then
    cache_wrapper="$(command -v "$cache_wrapper")" || return $?
    if command -v cygpath >/dev/null 2>&1; then cache_wrapper="$(cygpath -m "$cache_wrapper")"; fi
    export CODETERM_LANE_CACHE_WRAPPER="$cache_wrapper"
    export RUSTC_WRAPPER="$wrapper"
  fi
}

lane_locked_command() {
  local python
  if command -v python3 >/dev/null 2>&1 && python3 --version >/dev/null 2>&1; then python=python3
  elif command -v python >/dev/null 2>&1 && python --version >/dev/null 2>&1; then python=python
  else echo 'lane-check requires Python for the shared-intermediate lock' >&2; return 2; fi
  "$python" "$(dirname "${BASH_SOURCE[0]}")/lane-check-run.py" "$CARGO_BUILD_BUILD_DIR" "$@"
}

lane_cargo_check() {
  lane_locked_command cargo "$@"
}

lane_types_export() (
  [ "$#" = 0 ] || { echo 'Usage: ./build.sh types-export [--wait-build]' >&2; return 2; }
  configure_lane_build_dir || return $?
  configure_lane_cache || return $?
  configure_lane_workspace_wrapper || return $?
  lane_locked_command node scripts/export-chat-types.mjs
)

lane_check() (
  local packages=() tests=() timings=() target=(--lib) manifest=src-tauri/Cargo.toml package has_cli=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --tests) tests=(--tests) ;;
      --timings) timings=(--timings) ;;
      --*) echo 'Usage: ./build.sh lane-check <package>... [--tests] [--timings]' >&2; return 2 ;;
      *) packages+=("$1") ;;
    esac
    shift
  done
  [ "${#packages[@]}" -gt 0 ] || { echo 'lane-check requires a touched package' >&2; return 2; }
  for package in "${packages[@]}"; do
    case "$package" in
      domios-server|codeterm|codeterm-*) ;;
      *) echo 'lane-check requires a codeterm package' >&2; return 2 ;;
    esac
    [ "$package" != codeterm-cli ] || has_cli=1
  done
  if [ "$has_cli" = 1 ]; then
    [ "${#packages[@]}" = 1 ] || { echo 'lane-check cannot mix CLI and app workspace packages' >&2; return 2; }
    manifest=cli/Cargo.toml
    target=(--bin codeterm)
  fi
  local package_args=()
  for package in "${packages[@]}"; do package_args+=(-p "$package"); done
  configure_lane_build_dir || return $?
  configure_lane_cache || return $?
  configure_lane_workspace_wrapper || return $?
  prepare_dev_frontend_dist "$(pwd -P)"
  prepare_bundle_stubs
  lane_cargo_check check --manifest-path "$manifest" "${package_args[@]}" "${target[@]}" "${tests[@]}" "${timings[@]}" --message-format=json-diagnostic-rendered-ansi
)
