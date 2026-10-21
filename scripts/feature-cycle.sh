#!/usr/bin/env bash
# Caller: build.sh feature-cycle

feature_cycle_phase() {
  local label="$1" started status=0
  shift
  started="$(date +%s)"
  "$@" 2>&1 | tee "$CT_FEATURE_CYCLE_DIR/$label.log"
  status=${PIPESTATUS[0]}
  printf 'feature-cycle: phase=%s exit=%s wall=%ss\n' "$label" "$status" "$(( $(date +%s) - started ))" | tee -a "$CT_FEATURE_CYCLE_DIR/times.log"
  return "$status"
}

feature_cycle() {
  local mode="${1:?feature-cycle requires before or after}" selector="${2:?feature-cycle requires a test selector}"
  [ "$#" = 2 ] || { echo 'Usage: ./build.sh feature-cycle before|after <selector>' >&2; return 2; }
  case "$mode" in before|after) ;; *) return 2 ;; esac
  configure_dev_cargo
  prepare_dev_frontend_dist "$(pwd -P)"
  prepare_bundle_stubs
  source scripts/clippy-allow.sh
  CT_FEATURE_CYCLE_DIR="$CT_BUILD_ARTIFACT_DIR/feature-cycle-$mode"
  mkdir -p "$CT_FEATURE_CYCLE_DIR"
  local test_features=() product_args=() started
  if [ "$mode" = after ]; then
    test_features=(--no-default-features)
    configure_product_cargo -p codeterm -p codeterm-service-core
    product_args=("${CT_PRODUCT_CARGO_ARGS[@]}" --lib --bins)
  else
    product_args=(--all-targets)
  fi
  CODETERM_DEV=1 CODETERM_WEB_DIST="$(pwd -P)/dist" feature_cycle_phase dev-check \
    cargo check "${CT_DEV_CHECK_ARGS[@]}" --message-format=json || return $?
  started="$(date +%s)"
  feature_cycle_phase clippy cargo clippy --manifest-path src-tauri/Cargo.toml \
    -p codeterm -p codeterm-service-core "${product_args[@]}" --no-deps --message-format=json -- "${CLIPPY_ALLOW[@]}" || return $?
  if [ "$mode" = before ]; then
    CODETERM_DEV=1 CODETERM_WEB_DIST="$(pwd -P)/dist" feature_cycle_phase dev-lib \
      cargo check "${CT_DEV_CHECK_ARGS[@]}" --message-format=json || return $?
  fi
  feature_cycle_phase test-check cargo check --manifest-path src-tauri/Cargo.toml \
    -p codeterm-service-core --lib --tests --profile test "${test_features[@]}" --message-format=json || return $?
  printf 'feature-cycle: equivalent-check-queue mode=%s wall=%ss\n' "$mode" "$(( $(date +%s) - started ))" | tee -a "$CT_FEATURE_CYCLE_DIR/times.log"
  CARGO_TEST_CMD='cargo test --message-format json' CARGO_TEST_BUILD_CMD='cargo test --message-format json' \
    feature_cycle_phase test-one scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml \
      -p codeterm-service-core --lib "${test_features[@]}" "$selector" || return $?
  CT_BUILD_SOURCE_SHA="$(git rev-parse HEAD)" node "$CT_GATEWAY_SCRIPT_ROOT/scripts/feature-cycle-units.mjs" "$CT_FEATURE_CYCLE_DIR"
}
