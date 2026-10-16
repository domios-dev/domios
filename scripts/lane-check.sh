#!/usr/bin/env bash
# Caller: build.sh lane-check; scripts/tests/lane-check.test.mjs

configure_lane_build_dir() {
  local directory
  directory="$(CODETERM_RUST_CACHE_ROOT="${CODETERM_RUST_CACHE_ROOT:-}" CARGO_HOME="${CARGO_HOME:-}" \
    node "$(dirname "${BASH_SOURCE[0]}")/lane-build-dir.mjs")" || return $?
  export CARGO_BUILD_BUILD_DIR="$directory"
}

configure_lane_workspace_wrapper() {
  local directory wrapper
  directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || return $?
  wrapper="$directory/lane-rustc-wrapper.sh"
  case "${OSTYPE:-}" in msys*|cygwin*) wrapper="$(cygpath -m "$directory/lane-rustc-wrapper.cmd")" ;; esac
  # Cargo hashes the wrapper path for workspace units, while registry units stay shared.
  export RUSTC_WORKSPACE_WRAPPER="$wrapper"
}

lane_check() (
  local package="${1:?lane-check requires a touched package}" tests=() target=(--lib) manifest=src-tauri/Cargo.toml
  shift
  case "$package" in codeterm|codeterm-*) ;; *) echo 'lane-check requires a codeterm package' >&2; return 2 ;; esac
  if [ "${1:-}" = --tests ]; then tests=(--tests); shift; fi
  [ "$#" = 0 ] || { echo 'Usage: ./build.sh lane-check <package> [--tests]' >&2; return 2; }
  if [ "$package" = codeterm-cli ]; then
    manifest=cli/Cargo.toml
    target=(--bin codeterm)
  fi
  configure_lane_build_dir || return $?
  configure_lane_workspace_wrapper || return $?
  prepare_dev_frontend_dist "$(pwd -P)"
  prepare_bundle_stubs
  CARGO_BUILD_JOBS=4 cargo check --manifest-path "$manifest" -p "$package" "${target[@]}" "${tests[@]}"
)
