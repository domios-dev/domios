#!/usr/bin/env bash
# Caller: build.sh nextest-archive|nextest|test-one; scripts/nextest-archive.test.mjs

app_test_archive_path() {
  printf '%s/test-archives/%s/app.tar.zst\n' "$(ct_gateway_dir)" "$1"
}

find_app_test_archive() {
  local sha
  sha="$(git rev-parse HEAD)"
  CT_APP_TEST_ARCHIVE="$(app_test_archive_path "$sha")"
  [ -s "$CT_APP_TEST_ARCHIVE" ] || return 1
  git diff --quiet HEAD -- src-tauri .cargo || return 1
  configure_dev_cargo || return $?
  echo "nextest-archive: reuse sha=$sha archive=$CT_APP_TEST_ARCHIVE"
}

build_app_test_archive() {
  local sha="${1:-HEAD}" cw archive temporary started selector
  [ "$#" = 0 ] || shift
  if [ "$#" -gt 0 ]; then
    [ "$1" = --measure ] && [ "$#" -gt 1 ] || { echo 'Usage: ./build.sh nextest-archive [sha] [--measure selector...]' >&2; return 2; }
    shift
  fi
  sha="$(git rev-parse --verify "$sha^{commit}")"
  CT_BUILD_SOURCE_SHA="$sha"
  enter_cargo_gateway "nextest-archive $sha"
  cw="$(check_queue_worktree "$sha")"
  cd "$cw"
  enter_stable_build_path "$cw"
  configure_dev_cargo || return $?
  archive="$(app_test_archive_path "$sha")"
  if [ -s "$archive" ]; then
    echo "nextest-archive: reuse sha=$sha archive=$archive"
  else
  prepare_dev_frontend_dist "$cw"
  prepare_bundle_stubs
  mkdir -p "$(dirname "$archive")"
  temporary="${archive%.tar.zst}.$CT_BUILD_JOB_ID.tar.zst"
  started="$(date +%s)"
  CARGO_TARGET_DIR="$cw/target/app" cargo nextest archive --manifest-path src-tauri/Cargo.toml \
    -p codeterm-service-core -p codeterm --lib --cargo-profile test --archive-file "$temporary" || return $?
  [ -s "$temporary" ] || { echo 'ERROR: nextest archive produced no artifact' >&2; return 74; }
  mv "$temporary" "$archive"
  echo "nextest-archive: built sha=$sha wall=$(( $(date +%s) - started ))s archive=$archive"
  fi
  CT_APP_TEST_ARCHIVE="$archive"
  for selector in "$@"; do run_archived_app_test "$selector" || return $?; done
}

run_archived_app_test() {
  local filter="${1:?archived app test requires a selector}" started status=0
  started="$(date +%s)"
  local capped=scripts/test-capped.sh
  [ -z "${CT_GATEWAY_SCRIPT_ROOT:-}" ] || capped="$CT_GATEWAY_SCRIPT_ROOT/scripts/test-capped.sh"
  CARGO_TEST_CMD='cargo nextest run' CARGO_TEST_BUILD_CMD='' "$capped" \
    --archive-file "$CT_APP_TEST_ARCHIVE" --workspace-remap "$(pwd -P)" \
    -p codeterm-service-core --lib --no-tests=fail "$filter" || status=$?
  echo "nextest-archive: selector=$filter exit=$status wall=$(( $(date +%s) - started ))s"
  return "$status"
}
