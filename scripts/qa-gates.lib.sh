#!/usr/bin/env bash
# Internal QA gate registry + reporting. Sourced by scripts/qa-gate.sh.
set -u

QA_ROOT="${QA_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# Strip prod CodeTerm env leaked from orchestrator tabs (7685 + prod socket).
# QA_GATE_ENV_LOCKED=1 skips; non-prod CODETERM_API_PORT values are preserved.
qa_sanitize_env() {
  if [ "${QA_GATE_ENV_LOCKED:-0}" = "1" ]; then
    return 0
  fi
  case "${CODETERM_API_PORT:-}" in
    ''|7685) export CODETERM_API_PORT=7686 ;;
  esac
  case "${CODETERM_INSTANCE:-}" in
    prod|Prod|PROD) unset CODETERM_INSTANCE ;;
  esac
  if [ -n "${QA_GATE_INSTANCE:-}" ]; then
    export CODETERM_INSTANCE="$QA_GATE_INSTANCE"
  fi
  unset CODETERM_TAB_ID CODETERM_PARENT_TAB_ID 2>/dev/null || true
  export CODETERM_API_URL="http://127.0.0.1:${CODETERM_API_PORT}"
  if [ -n "${QA_GATE_SOCKET_PATH:-}" ]; then
    export CODETERM_SOCKET_PATH="$QA_GATE_SOCKET_PATH"
  else
    case "$(uname -s)" in
      MINGW*|MSYS*|CYGWIN*) export CODETERM_SOCKET_PATH='\\.\pipe\codeterm-dev.sock' ;;
      *) export CODETERM_SOCKET_PATH='codeterm-dev.sock' ;;
    esac
  fi
}

qa_host() {
  hostname -s 2>/dev/null || hostname 2>/dev/null || echo unknown
}

qa_branch() {
  git -C "$QA_ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown
}

qa_sha() {
  git -C "$QA_ROOT" rev-parse --short=12 HEAD 2>/dev/null || echo unknown
}

qa_kv_line() {
  local out=""
  local pair
  for pair in "$@"; do
    if [[ -z "$out" ]]; then out="$pair"; else out="$out $pair"; fi
  done
  printf '%s\n' "$out"
}

qa_emit() {
  local kind="$1"
  shift
  qa_kv_line "$kind" "$@"
}

qa_json_escape() {
  python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' <<<"$1"
}

# Portable timeout: prefers GNU timeout, else alarm via perl.
qa_run_timeout() {
  local secs="$1"
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout --signal=TERM --kill-after=30 "$secs" "$@"
    return $?
  fi
  if command -v gtimeout >/dev/null 2>&1; then
    gtimeout --signal=TERM --kill-after=30 "$secs" "$@"
    return $?
  fi
  perl -e 'alarm shift @ARGV; exec @ARGV' "$secs" "$@"
}

qa_dev_healthy() {
  local port="${CODETERM_API_PORT:-7686}"
  local status
  status="$(curl -sf "http://127.0.0.1:${port}/api/health" 2>/dev/null \
    | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null \
    || true)"
  [ "$status" = "ok" ]
}

qa_prep_stubs() {
  mkdir -p "$QA_ROOT/dist"
  node "$QA_ROOT/scripts/prepare-embed-stub.js" >/dev/null 2>&1 || true
  mkdir -p "$QA_ROOT/src-tauri/bundled-cli"
  [ -e "$QA_ROOT/src-tauri/bundled-cli/codeterm" ] \
    || touch "$QA_ROOT/src-tauri/bundled-cli/codeterm"
}

qa_clippy_allow() {
  CLIPPY_ALLOW=(
    -A unused-imports -A unused-mut -A unused-variables -A dead-code
    -A private-interfaces -A clippy::empty_line_after_doc_comments
    -A clippy::duplicated_attributes -A clippy::manual_abs_diff
    -A clippy::needless_range_loop -A clippy::redundant_closure
    -A clippy::new_without_default -A clippy::question_mark
    -A clippy::unnecessary_map_or -A clippy::single_char_add_str
    -A clippy::collapsible_else_if -A clippy::field_reassign_with_default
    -A clippy::items_after_test_module -A clippy::too_many_arguments
    -A clippy::type_complexity -A clippy::unnecessary_get_then_check
    -A clippy::ptr_arg -A clippy::needless_as_bytes
    -A clippy::single_element_loop -A clippy::bool_assert_comparison
    -A clippy::manual_repeat_n -A clippy::len_zero -A clippy::useless_vec
    -A clippy::needless_borrows_for_generic_args -A clippy::manual_clamp
    -A clippy::upper_case_acronyms -A clippy::needless_question_mark
    -A clippy::implicit_saturating_sub -A clippy::needless_borrow
    -A clippy::derivable_impls -A clippy::unnecessary_cast
    -A clippy::trim_split_whitespace -A clippy::should_implement_trait
    -A clippy::collapsible_str_replace -A clippy::missing_transmute_annotations
    -A clippy::doc_lazy_continuation -A clippy::manual_flatten
    -A clippy::for_kv_map -A clippy::manual_div_ceil
    -A clippy::doc_overindented_list_items -A clippy::useless_conversion
    -A clippy::explicit_auto_deref -A clippy::while_let_loop
    -A clippy::map_identity -A clippy::nonminimal_bool
    -A clippy::manual_pattern_char_comparison -A clippy::while_let_on_iterator
    -A clippy::unnecessary_unwrap -A clippy::single_match
    -A clippy::unwrap_or_default -A clippy::manual_split_once
    -A clippy::wrong_self_convention -A clippy::io_other_error
    -A clippy::manual_is_multiple_of -A clippy::clone_on_copy
    -A clippy::large_enum_variant -A clippy::manual_contains
    -A clippy::map_flatten -A clippy::print_literal
    -A clippy::cloned_ref_to_slice_refs -A clippy::filter_next
    -A clippy::needless_return -A clippy::collapsible_match
    -A clippy::unnecessary_sort_by -A unused_unsafe
  )
}

# gate_name -> default_timeout_secs
qa_gate_timeout() {
  case "$1" in
    shell-guards|fmt|file-sizes|comment-length) echo 120 ;;
    check) echo 600 ;;
    clippy|prepush-full) echo 1800 ;;
    rust-unit|cli-unit) echo 120 ;;
    ts-unit) echo 600 ;;
    plugin) echo 300 ;;
    test-all) echo 1200 ;;
    macos-native|linux-docker) echo 2400 ;;
    e2e-pytest) echo 600 ;;
    prepush-light) echo 180 ;;
    *) echo 600 ;;
  esac
}

qa_gate_platform() {
  case "$1" in
    macos-native)
      case "$(uname -s)" in Darwin) return 0 ;; *) return 1 ;; esac
      ;;
    linux-docker)
      command -v docker >/dev/null 2>&1 || return 1
      docker info >/dev/null 2>&1 || return 1
      return 0
      ;;
    *) return 0 ;;
  esac
}

qa_gate_needs_dev() {
  case "$1" in
    e2e-pytest|test-all) return 0 ;;
    *) return 1 ;;
  esac
}

qa_gate_desc() {
  case "$1" in
    shell-guards) echo "Mux-write + blocking-in-async + build-kill safety shell guards" ;;
    fmt) echo "cargo fmt --check (src-tauri + cli)" ;;
    check) echo "cargo check (src-tauri + cli)" ;;
    clippy) echo "cargo clippy -D warnings (src-tauri + cli)" ;;
    rust-unit) echo "src-tauri lib unit tests (capped)" ;;
    cli-unit) echo "cli crate unit tests (capped)" ;;
    ts-unit) echo "TypeScript unit tests (pnpm test:unit)" ;;
    plugin) echo "Plugin tests (pnpm test:plugins)" ;;
    test-all) echo "build.sh test:all aggregate (non-LLM layers)" ;;
    file-sizes) echo "Source file size discipline (--fail)" ;;
    comment-length) echo "Comment length discipline (--fail)" ;;
    macos-native) echo "macOS native lane (scripts/test-macos-native.sh)" ;;
    linux-docker) echo "Linux Docker lane (scripts/test-linux-docker.sh)" ;;
    e2e-pytest) echo "API e2e pytest (./build.sh e2e, dev required)" ;;
    prepush-light) echo "Lane pre-push hook (fmt + shell guards)" ;;
    prepush-full) echo "Integration pre-push hook (check + fmt + clippy)" ;;
    *) echo "unknown gate" ;;
  esac
}

qa_list_gates() {
  printf '%s\n' \
    shell-guards fmt check clippy rust-unit cli-unit ts-unit plugin \
    test-all file-sizes comment-length macos-native linux-docker \
    e2e-pytest prepush-light prepush-full
}

qa_run_gate_body() {
  local gate="$1"
  qa_sanitize_env
  cd "$QA_ROOT"
  case "$gate" in
    shell-guards)
      bash scripts/check-mux-write.sh
      bash scripts/check-blocking-in-async.sh
      bash scripts/check-build-kill-safety.sh
      ;;
    fmt)
      cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
      cargo fmt --manifest-path cli/Cargo.toml --all --check
      ;;
    check)
      qa_prep_stubs
      cargo check --manifest-path src-tauri/Cargo.toml
      cargo check --manifest-path cli/Cargo.toml
      ;;
    clippy)
      qa_prep_stubs
      qa_clippy_allow
      cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets \
        -- -D warnings "${CLIPPY_ALLOW[@]}"
      cargo clippy --manifest-path cli/Cargo.toml --workspace --all-targets \
        -- -D warnings "${CLIPPY_ALLOW[@]}"
      ;;
    rust-unit)
      qa_prep_stubs
      TEST_CAP_SECS="${QA_GATE_TEST_CAP:-90}" \
        bash scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib
      ;;
    cli-unit)
      TEST_CAP_SECS="${QA_GATE_TEST_CAP:-90}" \
        bash scripts/test-capped.sh --manifest-path cli/Cargo.toml
      ;;
    ts-unit)
      pnpm -s test:unit
      ;;
    plugin)
      pnpm -s test:plugins
      ;;
    test-all)
      ./build.sh test:all
      ;;
    file-sizes)
      bash scripts/check-file-sizes.sh --fail
      ;;
    comment-length)
      bash scripts/check-comment-length.sh --fail
      ;;
    macos-native)
      bash scripts/test-macos-native.sh
      ;;
    linux-docker)
      bash scripts/test-linux-docker.sh
      ;;
    e2e-pytest)
      ./build.sh e2e
      ;;
    prepush-light)
      PREPUSH_LIGHT=1 bash .githooks/pre-push <<<""
      ;;
    prepush-full)
      PREPUSH_FULL=1 bash .githooks/pre-push <<<""
      ;;
    *)
      echo "unknown gate: $gate" >&2
      return 2
      ;;
  esac
}
