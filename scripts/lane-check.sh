#!/usr/bin/env bash
# Caller: build.sh lane-check; scripts/tests/lane-check.test.mjs

configure_lane_build_dir() {
  local directory
  directory="$(CODETERM_RUST_CACHE_ROOT="${CODETERM_RUST_CACHE_ROOT:-}" CARGO_HOME="${CARGO_HOME:-}" \
    node "$(dirname "${BASH_SOURCE[0]}")/lane-build-dir.mjs")" || return $?
  export CARGO_BUILD_BUILD_DIR="$directory"
}

lane_check() {
  local package="${1:?lane-check requires a touched package}" tests=()
  shift
  case "$package" in codeterm|codeterm-*) ;; *) echo 'lane-check requires a codeterm package' >&2; return 2 ;; esac
  if [ "${1:-}" = --tests ]; then tests=(--tests); shift; fi
  [ "$#" = 0 ] || { echo 'Usage: ./build.sh lane-check <package> [--tests]' >&2; return 2; }
  prepare_dev_frontend_dist "$(pwd -P)"
  prepare_bundle_stubs
  CARGO_BUILD_JOBS=4 cargo check --manifest-path src-tauri/Cargo.toml -p "$package" --lib "${tests[@]}"
}
