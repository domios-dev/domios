#!/usr/bin/env bash
# Caller: build.sh lane-check; scripts/tests/lane-check.test.mjs

configure_lane_build_dir() {
  local root="${CODETERM_RUST_CACHE_ROOT:-}"
  if [ -n "$root" ]; then
    if command -v cygpath >/dev/null 2>&1; then root="$(cygpath -m "$root")"; fi
    export CARGO_BUILD_BUILD_DIR="${root%/}/lanes"
  else
    export CARGO_BUILD_BUILD_DIR="${CARGO_HOME:-$HOME/.cargo}/codeterm-lanes"
  fi
}

lane_check() (
  local package="${1:?lane-check requires a touched package}" tests=()
  shift
  case "$package" in codeterm|codeterm-*) ;; *) echo 'lane-check requires a codeterm package' >&2; return 2 ;; esac
  if [ "${1:-}" = --tests ]; then tests=(--tests); shift; fi
  [ "$#" = 0 ] || { echo 'Usage: ./build.sh lane-check <package> [--tests]' >&2; return 2; }
  configure_lane_build_dir
  prepare_dev_frontend_dist "$(pwd -P)"
  prepare_bundle_stubs
  CARGO_BUILD_JOBS=4 cargo check --manifest-path src-tauri/Cargo.toml -p "$package" --lib "${tests[@]}"
)
