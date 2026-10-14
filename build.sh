#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh dev-prebuilt --wait — restart a provenance-validated dev build without Cargo
#   ./build.sh check     — integration-owner cargo check
#   ./build.sh frontend  — production frontend typecheck + bundle
#   ./build.sh release-inputs — build target-aware frontend/CLI/embed bundle inputs
#   ./build.sh release-preflight — run local PR checks and verify release inputs
#   ./build.sh worktree-check — explicit isolated, nonincremental checkout check
#   ./build.sh build     — production build
#   ./build.sh install-local — incremental production build + self-install, then resume the calling tab
#   ./build.sh bootstrap — create/build bundled CLI + embed sidecar for fresh clones
#   ./build.sh qa-prep   — one-shot: dist + sidecars for a fresh isolated worktree (no tsc gate)
#   ./build.sh cli       — build CLI and install to the active dev data directory
#                          without waiting for the app/dev build gateway
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh e2e       — run non-LLM E2E tests against dev (port 7686)
#   ./build.sh e2e:llm   — run LLM E2E tests against dev (real provider tokens)
#   source build.sh env  — load MSVC env vars into current shell (Windows only)

set -e

source scripts/dev-notify.sh
dev_notify_capture_parent

CMD="${1:-dev}"
CT_DEV_WAIT=0
DEV_NOTIFY_TAB=""

# Uniform receipt: re-run under report_run_stream so one success or early-failure receipt
# (step, first error line, log path; exit 75 included) reaches --notify-tab / BUILD_NOTIFY_TAB.
case "$CMD" in
  check|check-tests|test-one|release-preflight|install-local|build|test|verify|fmt-check)
    if [ "${BUILD_NOTIFY_CHILD:-0}" != 1 ]; then
      BUILD_NOTIFY_GIVEN=0
      BUILD_NOTIFY_WANT_VALUE=0
      BUILD_NOTIFY_ARGS=()
      [ -z "${BUILD_NOTIFY_TAB+x}" ] || BUILD_NOTIFY_GIVEN=1
      for BUILD_NOTIFY_ARG in "${@:2}"; do
        if [ "$BUILD_NOTIFY_WANT_VALUE" = 1 ]; then
          BUILD_NOTIFY_TAB="$BUILD_NOTIFY_ARG"
          BUILD_NOTIFY_WANT_VALUE=0
          continue
        fi
        case "$BUILD_NOTIFY_ARG" in
          --notify-tab) BUILD_NOTIFY_WANT_VALUE=1; BUILD_NOTIFY_GIVEN=1 ;;
          --notify-tab=*) BUILD_NOTIFY_TAB="${BUILD_NOTIFY_ARG#--notify-tab=}"; BUILD_NOTIFY_GIVEN=1 ;;
          *) BUILD_NOTIFY_ARGS+=("$BUILD_NOTIFY_ARG") ;;
        esac
      done
      if [ "$BUILD_NOTIFY_GIVEN" = 1 ] && { [ "$BUILD_NOTIFY_WANT_VALUE" = 1 ] || [ -z "${BUILD_NOTIFY_TAB:-}" ]; }; then
        echo "ERROR: --notify-tab requires a non-empty tab id" >&2
        exit 2
      fi
      if [ "$BUILD_NOTIFY_GIVEN" = 1 ]; then
        if [ -z "$DEV_NOTIFY_PARENT_CLI" ]; then
          echo "ERROR: codeterm was not available on the parent PATH for --notify-tab" >&2
          exit 127
        fi
        BUILD_NOTIFY_LOG="$(mktemp "${TMPDIR:-/tmp}/codeterm-build-$CMD.XXXXXX")"
        BUILD_NOTIFY_MARKER="$BUILD_NOTIFY_LOG.sent"
        rm -rf "$BUILD_NOTIFY_MARKER"
        BUILD_NOTIFY_STATUS=0
        report_run_stream "$BUILD_NOTIFY_MARKER" "$BUILD_NOTIFY_TAB" "$CMD" "$CMD ${BUILD_NOTIFY_ARGS[*]:-}" "$BUILD_NOTIFY_LOG" \
          env BUILD_NOTIFY_CHILD=1 bash "$0" "$CMD" ${BUILD_NOTIFY_ARGS[@]+"${BUILD_NOTIFY_ARGS[@]}"} || BUILD_NOTIFY_STATUS=$?
        if [ "$BUILD_NOTIFY_STATUS" -eq 0 ]; then
          BUILD_NOTIFY_BODY="$(dev_notify_new_body_file)"
          printf 'OK label=%s step=%s\nLog: %s\n' "$CMD" "$CMD ${BUILD_NOTIFY_ARGS[*]:-}" "$BUILD_NOTIFY_LOG" > "$BUILD_NOTIFY_BODY"
          dev_notify_send_once "$BUILD_NOTIFY_MARKER" "$BUILD_NOTIFY_TAB" "$BUILD_NOTIFY_BODY" || true
        fi
        rm -rf "$BUILD_NOTIFY_MARKER"
        exit "$BUILD_NOTIFY_STATUS"
      fi
    fi
    ;;
esac
if [ "$CMD" = "dev" ] || [ "$CMD" = "dev-prebuilt" ]; then
  DEV_NOTIFY_ARGS=("${@:2}")
  DEV_NOTIFY_INDEX=0
  DEV_NOTIFY_SEEN=0
  while [ "$DEV_NOTIFY_INDEX" -lt "${#DEV_NOTIFY_ARGS[@]}" ]; do
    DEV_NOTIFY_ARG="${DEV_NOTIFY_ARGS[$DEV_NOTIFY_INDEX]}"
    case "$DEV_NOTIFY_ARG" in
      --wait|--wait-build)
        CT_DEV_WAIT=1
        DEV_NOTIFY_INDEX=$((DEV_NOTIFY_INDEX + 1))
        ;;
      --notify-tab)
        DEV_NOTIFY_VALUE_INDEX=$((DEV_NOTIFY_INDEX + 1))
        if [ "$DEV_NOTIFY_VALUE_INDEX" -ge "${#DEV_NOTIFY_ARGS[@]}" ]; then
          DEV_NOTIFY_VALUE=""
        else
          DEV_NOTIFY_VALUE="${DEV_NOTIFY_ARGS[$DEV_NOTIFY_VALUE_INDEX]}"
        fi
        if [ -z "$DEV_NOTIFY_VALUE" ] || [[ "$DEV_NOTIFY_VALUE" == --* ]]; then
          echo "ERROR: --notify-tab requires a non-empty tab id" >&2
          exit 2
        fi
        if [ "$DEV_NOTIFY_SEEN" = "1" ]; then
          echo "ERROR: --notify-tab may be supplied only once" >&2
          exit 2
        fi
        DEV_NOTIFY_TAB="$DEV_NOTIFY_VALUE"
        DEV_NOTIFY_SEEN=1
        DEV_NOTIFY_INDEX=$((DEV_NOTIFY_INDEX + 2))
        ;;
      --notify-tab=*)
        DEV_NOTIFY_VALUE="${DEV_NOTIFY_ARG#--notify-tab=}"
        if [ -z "$DEV_NOTIFY_VALUE" ]; then
          echo "ERROR: --notify-tab requires a non-empty tab id" >&2
          exit 2
        fi
        if [ "$DEV_NOTIFY_SEEN" = "1" ]; then
          echo "ERROR: --notify-tab may be supplied only once" >&2
          exit 2
        fi
        DEV_NOTIFY_TAB="$DEV_NOTIFY_VALUE"
        DEV_NOTIFY_SEEN=1
        DEV_NOTIFY_INDEX=$((DEV_NOTIFY_INDEX + 1))
        ;;
      *) DEV_NOTIFY_INDEX=$((DEV_NOTIFY_INDEX + 1)) ;;
    esac
  done
  if [ "$DEV_NOTIFY_SEEN" = "1" ]; then
    if [ -z "$DEV_NOTIFY_PARENT_CLI" ]; then
      echo "ERROR: codeterm was not available on the parent PATH for --notify-tab" >&2
      exit 127
    fi
    DEV_NOTIFY_SENT_MARKER="${DEV_NOTIFY_SENT_MARKER:-${TMPDIR:-/tmp}/codeterm-dev-notify.$$.sent}"
    if [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ]; then rm -rf "$DEV_NOTIFY_SENT_MARKER"; fi
    DEV_NOTIFY_STAGE=setup
    trap 'dev_notify_exit_report "$?"' EXIT
  fi
fi

if [ -n "$DEV_NOTIFY_TAB" ] && [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ]; then
  DEV_NOTIFY_STREAM_LOG="$(mktemp "${TMPDIR:-/tmp}/codeterm-dev-stream.XXXXXX")"
  DEV_NOTIFY_EARLY_MARKER="$DEV_NOTIFY_SENT_MARKER"
  trap - EXIT
  DEV_NOTIFY_STREAM_STATUS=0
  report_run_stream "$DEV_NOTIFY_EARLY_MARKER" "$DEV_NOTIFY_TAB" "$CMD" startup "$DEV_NOTIFY_STREAM_LOG" \
    env DEV_NOTIFY_STREAM_CHILD=1 DEV_NOTIFY_SENT_MARKER="$DEV_NOTIFY_SENT_MARKER" bash "$0" "$@" || DEV_NOTIFY_STREAM_STATUS=$?
  rm -rf "$DEV_NOTIFY_EARLY_MARKER"
  exit "$DEV_NOTIFY_STREAM_STATUS"
fi

# --- Load .env (build secrets like UPDATER_TOKEN) ---
if [ -f ".env" ]; then
  set -a
  source .env
  set +a
fi

# --- MSVC environment (Windows only) ---
CT_MSVC_HOST_ARCH="x64"

# Every cargo invocation this script starts inherits CARGO_BUILD_TARGET, so the
# link libraries must follow that target, not the host.
ct_msvc_target_arch() {
  case "${CARGO_BUILD_TARGET:-}" in
    aarch64-*) echo "arm64" ;;
    i686-*)    echo "x86" ;;
    *)         echo "$CT_MSVC_HOST_ARCH" ;;
  esac
}

# A host-arch LIB entry under a cross target links arm64 objects against x64
# import libs and fails as LNK1120 unresolved externals, never as an arch error.
ct_assert_msvc_lib_arch() {
  local want="$1" entry leaf saved_ifs="$IFS"
  IFS=';'
  for entry in $LIB; do
    IFS="$saved_ifs"
    [ -n "$entry" ] || continue
    leaf="${entry##*/}"
    leaf="${leaf##*\\}"
    if [ "$leaf" != "$want" ]; then
      echo "ERROR: LIB entry '$entry' is not $want — refusing to link $want objects against host libraries" >&2
      return 1
    fi
    IFS=';'
  done
  IFS="$saved_ifs"
}

ct_msvc_build_tools_base() {
  local preferred="$1" vswhere="$2" discovered
  if [ -d "$preferred/VC/Tools/MSVC" ]; then
    printf '%s\n' "$preferred"
    return
  fi
  discovered=""
  if [ -f "$vswhere" ]; then
    discovered="$("$vswhere" -latest -products '*' -version '[17.0,18.0)' \
      -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
      -property installationPath | tr -d '\r' | tr '\\' '/')" || discovered=""
  fi
  if [ -n "$discovered" ] && [ -d "$discovered/VC/Tools/MSVC" ]; then
    printf '%s\n' "$discovered"
  else
    printf '%s\n' "$preferred"
  fi
}

setup_env() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    BT_BASE="$(ct_msvc_build_tools_base \
      "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools" \
      "C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe")"
    SDK_BASE="C:/Program Files (x86)/Windows Kits/10"
    MSVC_VER=$(ls -1 "$BT_BASE/VC/Tools/MSVC" 2>/dev/null | sort -V | tail -1)
    SDK_VER=$(ls -1 "$SDK_BASE/Include" 2>/dev/null | grep -E '^10\.' | sort -V | tail -1)
    if [ -z "$MSVC_VER" ] || [ -z "$SDK_VER" ]; then
      echo "ERROR: MSVC ($BT_BASE) or Windows SDK ($SDK_BASE) not found" >&2
      return 1
    fi
    local vc_root="$BT_BASE/VC/Tools/MSVC/$MSVC_VER"
    local target_arch host_bin
    target_arch="$(ct_msvc_target_arch)"
    host_bin="$vc_root/bin/Hostx64/$CT_MSVC_HOST_ARCH"
    export CC="$host_bin/cl.exe"
    export CXX="$host_bin/cl.exe"
    if [ "$target_arch" != "$CT_MSVC_HOST_ARCH" ]; then
      local cross_bin="$vc_root/bin/Hostx64/$target_arch" cc_key
      if [ ! -f "$cross_bin/cl.exe" ]; then
        echo "ERROR: cross cl.exe not found at $cross_bin (VC.Tools.$target_arch missing?)" >&2
        return 1
      fi
      cc_key="${CARGO_BUILD_TARGET//-/_}"
      export "CC_$cc_key=$cross_bin/cl.exe"
      export "CXX_$cc_key=$cross_bin/cl.exe"
      export "AR_$cc_key=$cross_bin/lib.exe"
    fi
    export LIB="$vc_root/lib/$target_arch;$SDK_BASE/Lib/$SDK_VER/ucrt/$target_arch;$SDK_BASE/Lib/$SDK_VER/um/$target_arch"
    export INCLUDE="$vc_root/include;$SDK_BASE/Include/$SDK_VER/ucrt;$SDK_BASE/Include/$SDK_VER/um;$SDK_BASE/Include/$SDK_VER/shared"
    export RC="$SDK_BASE/bin/$SDK_VER/$CT_MSVC_HOST_ARCH/rc.exe"
    ct_assert_msvc_lib_arch "$target_arch" || return 1
    export PATH="$(cygpath -u "$host_bin"):$(cygpath -u "$SDK_BASE/bin/$SDK_VER/$CT_MSVC_HOST_ARCH"):$PATH"
    export npm_config_script_shell="$(cygpath -w "$(command -v bash)")"
    echo "MSVC env loaded (VS 2022 at $BT_BASE, MSVC $MSVC_VER, link libraries $target_arch)"
  fi
}

export CODETERM_FRONTEND_BUILD_HASH="$(git rev-parse --short HEAD 2>/dev/null || true)"

# --- Kill THIS dev instance only (never prod, never another worktree) ---
# Scoped strictly to $CT_PORT / $CT_VITE_PORT / this instance's screen+pidfile.
# No global pgrep on target/debug/codeterm — that would take down every worktree.
kill_dev() {
  if [ "$CT_PORT" = "7685" ]; then
    echo "ERROR: refusing to kill prod CodeTerm port 7685" >&2
    exit 1
  fi
  if [ "${CODETERM_TEST_KILL_DRY_RUN:-0}" = "1" ]; then
    echo "Dry-run dev kill instance='${CT_INSTANCE:-default}' api=$CT_PORT vite=$CT_VITE_PORT cdp=$CT_CDP_PORT identifier=$CT_IDENTIFIER"
    return
  fi
  if [ "${CODETERM_TEST_KILL_CLEAN_REQUEST_ONLY:-0}" = "1" ]; then
    request_dev_clean_restart 2>/dev/null || true
    return
  fi
  local session_prefix="codeterm-dev${CT_SUFFIX}-"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    local dev_pids
    dev_pids="$(lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null || true)"
    if [ -n "$dev_pids" ]; then
      request_dev_clean_restart 2>/dev/null || true
      local graceful=1
      while IFS= read -r dev_pid; do
        wait_for_dev_exit "$dev_pid" || graceful=0
      done <<< "$dev_pids"
      if [ "$graceful" != "1" ]; then
        echo "Clean shutdown deadline elapsed; force-killing dev instance on port $CT_PORT..."
        echo "$dev_pids" | xargs kill -9 2>/dev/null || true
      fi
    fi
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    if command -v screen >/dev/null 2>&1; then
      screen -ls 2>/dev/null | awk -v p="$session_prefix" 'index($1,p){print $1}' | while read -r session; do
        screen -S "$session" -X quit 2>/dev/null || true
      done
    fi
    local vite_pids
    vite_pids="$(lsof -ti:"$CT_VITE_PORT" 2>/dev/null || true)"
    [ -z "$vite_pids" ] || echo "$vite_pids" | xargs kill -9 2>/dev/null || true
    if [ -f "$CT_DATA_DIR/daemon.pid" ]; then
      kill "$(cat "$CT_DATA_DIR/daemon.pid")" 2>/dev/null || true
      rm -f "$CT_DATA_DIR/daemon.pid"
    fi
  else
    local dev_pid
    dev_pid=$(netstat -ano 2>/dev/null | grep ":$CT_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
    if [ -n "$dev_pid" ] && [ "$dev_pid" != "0" ]; then
      request_dev_clean_restart 2>/dev/null || true
      if ! wait_for_dev_exit "$dev_pid"; then
        echo "Clean shutdown deadline elapsed; force-killing dev instance (PID $dev_pid on port $CT_PORT)..."
        taskkill //PID "$dev_pid" //T //F 2>/dev/null || true
      fi
    fi
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    local vite_pid
    vite_pid=$(netstat -ano 2>/dev/null | grep ":$CT_VITE_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
    if [ -n "$vite_pid" ] && [ "$vite_pid" != "0" ]; then
      taskkill //PID "$vite_pid" //T //F 2>/dev/null || true
    fi
    windows_dev_leftover_cleanup
  fi
  node scripts/reap-dev-sidecars.mjs "$CT_DATA_DIR"
}

request_dev_clean_restart() {
  local config="$CT_DATA_DIR/config.toml"
  [ -f "$config" ] || return 1

  local token
  token=$(grep -E '^token = ' "$config" | head -1 | sed 's/.*"\(.*\)"/\1/')
  [ -n "$token" ] || return 1

  curl --fail --silent --show-error --max-time 3 \
    -H "Authorization: Bearer $token" \
    -X POST "http://127.0.0.1:$CT_PORT/api/debug/shutdown/clean-restart" \
    >/dev/null
}

wait_for_dev_exit() {
  local pid="$1"
  if [[ "$OSTYPE" == linux* ]]; then
    timeout 10 bash -c 'while kill -0 "$1" 2>/dev/null; do sleep 0.1; done' -- "$pid"
  elif [[ "$OSTYPE" == darwin* ]]; then
    perl -e '$pid = shift; $SIG{ALRM} = sub { exit 1 }; alarm 10; while (kill 0, $pid) { select undef, undef, undef, 0.1 }' "$pid"
  else
    powershell.exe -NoProfile -Command "try { \$devProcess = [System.Diagnostics.Process]::GetProcessById($pid) } catch [System.ArgumentException] { exit 0 } catch { exit 1 }; if (\$devProcess.WaitForExit(10000)) { exit 0 }; if (\$devProcess.HasExited) { exit 0 }; exit 1" >/dev/null 2>&1
  fi
}

CMD="${1:-dev}"
run_workspace_lib_tests() {
  local package
  local test_cap_secs="${TEST_CAP_SECS:-3600}"
  for package in codeterm codeterm-account codeterm-billing codeterm-frontend-bundle codeterm-types codeterm-wire codeterm-sentinel codeterm-decision codeterm-provider-signals codeterm-util codeterm-mesh codeterm-mem codeterm-plan codeterm-bridge codeterm-pty codeterm-plugin codeterm-plugins; do
    TEST_CAP_SECS="$test_cap_secs" scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p "$package" --lib
  done
}

run_nextest_scope() {
  [ "$#" -le 2 ] || {
    echo "usage: ./build.sh nextest <scope> [filterset]" >&2
    return 2
  }
  local scope="${1:?nextest requires a scope}"
  local filterset="${2:-}"
  local manifest="src-tauri/Cargo.toml"
  local package=""
  local targets=()
  local filter_args=()

  case "$scope" in
    app|codeterm) package="codeterm"; targets+=(--lib --no-default-features) ;;
    input-platform) targets+=(--lib -p codeterm-input-macos -p codeterm-input-windows) ;;
    frontend-bundle|codeterm-frontend-bundle) package="codeterm-frontend-bundle"; targets+=(--lib) ;;
    notification-core|codeterm-notification-core) package="codeterm-notification-core"; targets+=(--lib) ;;
    analytics|codeterm-analytics) package="codeterm-analytics"; targets+=(--lib) ;;
    agent-detect|codeterm-agent-detect) package="codeterm-agent-detect"; targets+=(--lib) ;;
    billing|codeterm-billing) package="codeterm-billing"; targets+=(--lib) ;;
    usage|codeterm-usage) package="codeterm-usage"; targets+=(--lib) ;;
    account|codeterm-account) package="codeterm-account"; targets+=(--lib) ;;
    repowatch|codeterm-repowatch) package="codeterm-repowatch"; targets+=(--lib) ;;
    plan|codeterm-plan) package="codeterm-plan"; targets+=(--lib) ;;
    pty|codeterm-pty) package="codeterm-pty" ;;
    util|codeterm-util) package="codeterm-util"; targets+=(--lib) ;;
    browser|codeterm-browser) package="codeterm-browser"; targets+=(--lib) ;;
    types|codeterm-types) package="codeterm-types"; targets+=(--all-targets) ;;
    wire|codeterm-wire) package="codeterm-wire"; targets+=(--all-targets) ;;
    sentinel|codeterm-sentinel) package="codeterm-sentinel"; targets+=(--all-targets --features test-support) ;;
    decision|codeterm-decision) package="codeterm-decision"; targets+=(--all-targets) ;;
    provider-signals|codeterm-provider-signals) package="codeterm-provider-signals"; targets+=(--all-targets) ;;
    mem|codeterm-mem) package="codeterm-mem"; targets+=(--lib) ;;
    mesh|codeterm-mesh) package="codeterm-mesh"; targets+=(--lib) ;;
    plugin|codeterm-plugin) package="codeterm-plugin"; targets+=(--lib) ;;
    plugins|codeterm-plugins) package="codeterm-plugins"; targets+=(--lib) ;;
    cli|codeterm-cli) manifest="cli/Cargo.toml"; package="codeterm-cli"; targets+=(--all-targets) ;;
    pty-fork) manifest="src-tauri/pty-fork/Cargo.toml" ;;
    codeterm-*) package="$scope"; targets+=(--lib) ;;
    *)
      echo "Unknown nextest scope: $scope" >&2
      return 2
      ;;
  esac

  enter_build_gateway "$CMD $scope${filterset:+ $filterset}"
  if [ "$scope" = "app" ] || [ "$scope" = "codeterm" ]; then
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    prepare_exceptional_worktree_target
  fi

  if [ -n "$filterset" ]; then
    filter_args+=(--filter-expr "$filterset")
  fi
  if [ -n "$package" ]; then
    if [[ "$filterset" == *"rdeps("* ]]; then
      targets+=(--workspace)
    else
      targets+=(-p "$package")
    fi
  fi

  local timing_dir run_exit=0 guard_exit=0 alone_exit=0
  timing_dir="$(mktemp -d)"
  if cargo nextest list --manifest-path "$manifest" "${targets[@]}" \
    --list-type binaries-only --message-format json > "$timing_dir/binaries.json"; then
    :
  else
    run_exit=$?
    rm -rf "$timing_dir"
    return "$run_exit"
  fi
  if cargo metadata --manifest-path "$manifest" --format-version 1 --locked > "$timing_dir/cargo.json"; then
    :
  else
    run_exit=$?
    rm -rf "$timing_dir"
    return "$run_exit"
  fi
  if CARGO_TEST_CMD="cargo nextest run" CARGO_TEST_BUILD_CMD="" NEXTEST_STATUS_LEVEL=pass \
    scripts/test-capped.sh \
    --binaries-metadata "$timing_dir/binaries.json" --cargo-metadata "$timing_dir/cargo.json" \
    "${filter_args[@]}" --no-fail-fast --no-tests=fail 2>&1 | tee "$timing_dir/parallel.log"; then
    run_exit=${PIPESTATUS[0]}
  else
    run_exit=${PIPESTATUS[0]}
  fi
  if [ "${SLOW_UNIT_TESTS_MEASUREMENT:-}" = "alone" ]; then
    if [ "${NEXTEST_TEST_THREADS:-}" != "1" ]; then
      echo "alone measurement requires NEXTEST_TEST_THREADS=1" >&2
      guard_exit=2
    elif node scripts/check-slow-unit-tests.mjs "$timing_dir/parallel.log" --alone "$timing_dir/parallel.log"; then
      :
    else
      guard_exit=$?
    fi
  elif node scripts/check-slow-unit-tests.mjs "$timing_dir/parallel.log" --candidates "$timing_dir/candidates.json"; then
    if [ "$(node -e 'process.stdout.write(String(JSON.parse(require("fs").readFileSync(process.argv[1])).entries.length))' "$timing_dir/candidates.json")" != "0" ]; then
      if node scripts/confirm-slow-unit-tests.mjs "$timing_dir/candidates.json" \
        "$timing_dir/binaries.json" "$timing_dir/cargo.json" "$timing_dir/alone.log"; then
        :
      else
        alone_exit=$?
      fi
      if node scripts/check-slow-unit-tests.mjs "$timing_dir/parallel.log" --alone "$timing_dir/alone.log"; then
        :
      else
        guard_exit=$?
      fi
    elif node scripts/check-slow-unit-tests.mjs "$timing_dir/parallel.log"; then
      :
    else
      guard_exit=$?
    fi
  else
    guard_exit=$?
  fi
  rm -rf "$timing_dir"
  [ "$run_exit" -eq 0 ] || return "$run_exit"
  [ "$alone_exit" -eq 0 ] || return "$alone_exit"
  return "$guard_exit"
}

case "$CMD" in
  gateway-*|help|gc|cache-status|cache-verify|cache-stats|build-history) ;;
  *) setup_env ;;
esac

# --- Per-worktree dev instance isolation ---
# CODETERM_INSTANCE (optional) names a dev instance so multiple worktrees can
# run ./build.sh dev concurrently without colliding. Unset = canonical defaults
# (.codeterm-dev / 7686 / 1420). See AGENTS.md "Process safety".
CT_INSTANCE="${CODETERM_INSTANCE:-}"
CT_PORT="${CODETERM_API_PORT:-7686}"
CT_VITE_PORT="${CODETERM_VITE_PORT:-}"
CT_CDP_PORT="${CODETERM_CDP_PORT:-}"
case "$CMD" in
  dev|dev-prebuilt|kill|e2e|e2e:llm)
    if [ "$CT_PORT" = "7685" ]; then
      if [ -n "$CT_INSTANCE" ]; then
        echo "ERROR: refusing to run $CMD against prod port 7685 (CODETERM_INSTANCE=$CT_INSTANCE)." >&2
        echo "Pick a non-prod CODETERM_API_PORT for this dev instance." >&2
        exit 2
      fi
      echo "WARNING: ignoring inherited CODETERM_API_PORT=7685 for $CMD; using dev port 7686." >&2
      CT_PORT=7686
    fi
    ;;
esac
if [ -n "$CT_INSTANCE" ]; then
  CT_SLUG=$(printf '%s' "$CT_INSTANCE" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g')
  CT_SUFFIX="-$CT_SLUG"
  CT_IDENTIFIER="com.codeterm.dev.$(printf '%s' "$CT_SLUG" | tr '-' '.')"
  # Stable per-instance vite port: sum of slug bytes mod 200, +1 offset.
  CT_OFFSET=$(printf '%s' "$CT_SLUG" | od -An -tu1 | awk '{for(i=1;i<=NF;i++)s+=$i} END{print (s%200)+1}')
  CT_VITE_PORT="${CT_VITE_PORT:-$((1420 + CT_OFFSET))}"
  CT_CDP_PORT="${CT_CDP_PORT:-$((9223 + CT_OFFSET))}"
else
  CT_SLUG=""
  CT_SUFFIX=""
  CT_IDENTIFIER="com.codeterm.dev"
  CT_VITE_PORT="${CT_VITE_PORT:-1420}"
  CT_CDP_PORT="${CT_CDP_PORT:-9223}"
fi
CT_DATA_DIR="$HOME/.codeterm-dev${CT_SUFFIX}"
DEV_NOTIFY_LOG="/tmp/codeterm-dev${CT_SUFFIX}.log"
# Export so the spawned daemon + its child PTYs inherit the instance identity.
export CODETERM_INSTANCE="$CT_INSTANCE"
export CODETERM_API_PORT="$CT_PORT"
export CODETERM_VITE_PORT="$CT_VITE_PORT"
export CODETERM_DEV_IDENTIFIER="$CT_IDENTIFIER"
export CODETERM_CDP_PORT="$CT_CDP_PORT"

if [ "$CMD" = "dev" ] || [ "$CMD" = "dev-prebuilt" ]; then
  for arg in "${@:2}"; do
    case "$arg" in
      --release-gateway-after-ready)
        echo "ERROR: --release-gateway-after-ready is obsolete; named dev --wait always skips the shared gateway handoff" >&2
        exit 2
        ;;
    esac
  done
else
  BUILD_ARGS=()
  for arg in "${@:2}"; do
    if [ "$arg" = "--wait-build" ] && [ "$CMD" = "check-tests" ]; then
      echo "ERROR: check-tests does not use the build gateway, so --wait-build has no effect; run ./build.sh check-tests without it" >&2
      exit 2
    elif [ "$arg" = "--wait-build" ]; then
      export CODETERM_BUILD_GATEWAY_MODE=queue
    else
      BUILD_ARGS+=("$arg")
    fi
  done
  set -- "$CMD" "${BUILD_ARGS[@]}"
  export CODETERM_BUILD_GATEWAY_MODE="${CODETERM_BUILD_GATEWAY_MODE:-fail-fast}"
fi

named_dev_target_dir() {
  printf '%s\n' "$CT_DATA_DIR/target"
}

named_dev_wait_isolated() {
  [ -n "$CT_INSTANCE" ] && [ "$CT_DEV_WAIT" = "1" ]
}

prepare_named_dev_target() {
  export CARGO_TARGET_DIR="$(named_dev_target_dir)"
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  echo "[isolated-dev] cargo target=$CARGO_TARGET_DIR"
}

if [ "${CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY:-0}" = "1" ]; then
  if [ -n "$CT_INSTANCE" ]; then
    printf 'instance=%s\nshared_gateway_handoff=skipped\n' "$CT_INSTANCE"
  else
    printf 'instance=\nshared_gateway_handoff=required\n'
  fi
  if [ -n "$DEV_NOTIFY_TAB" ]; then
    printf 'notify_tab=%s\nparent_cli=%s\n' "$DEV_NOTIFY_TAB" "$DEV_NOTIFY_PARENT_CLI"
  fi
  if [ "${CODETERM_TEST_SEND_PARENT_REPORT:-0}" = "1" ] && [ -n "$DEV_NOTIFY_TAB" ]; then
    dev_notify_ready_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "${CT_INSTANCE:-}" "${CT_PORT:-7686}" \
      aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 "$(pwd -P)"
  fi
  exit 0
fi

if [ "${CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY:-0}" = "1" ]; then
  if named_dev_wait_isolated; then
    printf 'mode=isolated-named-wait\ntarget=%s\nbuild_gateway=skipped\n' "$(named_dev_target_dir)"
  else
    printf 'mode=shared\nbuild_gateway=required\n'
  fi
  exit 0
fi

# A cold prebuilt refusal must be read-only; every other command keeps the
# existing hook setup behavior.
if [ "$CMD" != "dev-prebuilt" ] && [ "$CMD" != "cache-status" ] && [ "$CMD" != "cache-verify" ] && [ "$CMD" != "cache-stats" ] && [ "$CMD" != build-history ]; then
  git config core.hooksPath .githooks 2>/dev/null || true
fi

source scripts/cache-policy.sh
source scripts/build-gateway.sh
source scripts/release-signing.sh

if [ -n "${CODETERM_BUILD_GATEWAY_DIR:-}" ]; then
  echo "ERROR: CODETERM_BUILD_GATEWAY_DIR cannot redirect the machine build gateway" >&2
  exit 2
fi
if [ -n "${CT_BUILD_GATEWAY_SCOPE:-}" ]; then
  echo "ERROR: CT_BUILD_GATEWAY_SCOPE is internal to fake gateway commands" >&2
  exit 2
fi
case "$CMD" in
  gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe|gateway-log-probe)
    [ -z "${CODETERM_BUILD_GATEWAY_FIXTURE_DIR:-}" ] || ct_gateway_set_fixture_dir "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR"
    ;;
  *)
    if [ -n "${CODETERM_BUILD_GATEWAY_FIXTURE_DIR:-}" ]; then
      echo "ERROR: fixture gateway scope is unavailable to production orchestration command '$CMD'" >&2
      exit 2
    fi
    ;;
esac

CT_BUILD_GATEWAY_LOGGING_FAILURE=74

finish_build_gateway_logging() {
  local reader_status=0 watchdog_pid reader_start deadline=10000
  exec 1>&3 2>&4
  if [ -n "${CT_BUILD_GATEWAY_LOG_READER_STATUS:-}" ]; then
    reader_status="$CT_BUILD_GATEWAY_LOG_READER_STATUS"
  else
    if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = fixture ]; then
      deadline="${CT_BUILD_GATEWAY_READER_TIMEOUT_MS:-10000}"
    fi
    reader_start="$(ct_gateway_process_start_token "$CT_BUILD_GATEWAY_LOG_READER_PID" 2>/dev/null || true)"
    node scripts/build-gateway-reader-deadline.mjs "$CT_BUILD_GATEWAY_LOG_READER_PID" "$reader_start" "$deadline" &
    watchdog_pid=$!
    if wait "$CT_BUILD_GATEWAY_LOG_READER_PID"; then reader_status=0; else reader_status=$?; fi
    kill "$watchdog_pid" 2>/dev/null || true
    wait "$watchdog_pid" 2>/dev/null || true
  fi
  rm -f -- "${CT_BUILD_GATEWAY_LOG_FIFO:-}"
  exec 3>&- 4>&-
  return "$reader_status"
}

complete_build_gateway_before_install_handoff() {
  local reader_status=0 finish_status=0
  trap - EXIT INT TERM
  if [ -n "${CT_BUILD_GATEWAY_LOG_PATH:-}" ]; then
    printf 'Build gateway job %s build exit: 0\n' "$CT_BUILD_JOB_ID"
    finish_build_gateway_logging || reader_status=$?
    if [ "$reader_status" -ne 0 ]; then
      printf 'ERROR: build gateway logging path broke before installer handoff (exit %s); log: %s\n' \
        "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
      ct_gateway_finish "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "0" logging_failed
      return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
    fi
  fi
  ct_gateway_finish 0 0 || finish_status=$?
  return "$finish_status"
}

build_gateway_logging_on_exit() {
  local captured_status="$?" command_status="" command_status_text=UNKNOWN
  local summary_status reader_status finish_status
  trap - EXIT INT TERM
  set +e
  if [ "${CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN:-0}" = "1" ] && [ "${CT_GATEWAY_CANCELLED:-0}" != "1" ]; then
    command_status="$captured_status"
    command_status_text="$captured_status"
  fi
  printf 'Build gateway job %s exit: %s\n' "$CT_BUILD_JOB_ID" "$command_status_text"
  summary_status=$?
  finish_build_gateway_logging
  reader_status=$?
  if [ "$summary_status" -ne 0 ] || [ "$reader_status" -ne 0 ]; then
    printf 'ERROR: build gateway logging path broke (exit %s); command exit was %s; log: %s\n' \
      "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "$command_status_text" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
    ct_gateway_finish "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "$command_status" logging_failed
    finish_status=$?
    [ "$finish_status" -eq 0 ] || exit "$finish_status"
    exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  ct_gateway_finish "$captured_status" "$command_status"
  finish_status=$?
  [ "$finish_status" -eq 0 ] || exit "$finish_status"
  exit "$captured_status"
}

break_build_gateway_log_reader() {
  [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = "fixture" ] || return 2
  kill -KILL "$CT_BUILD_GATEWAY_LOG_READER_PID"
  if wait "$CT_BUILD_GATEWAY_LOG_READER_PID"; then
    CT_BUILD_GATEWAY_LOG_READER_STATUS=0
  else
    CT_BUILD_GATEWAY_LOG_READER_STATUS=$?
  fi
  return 0
}

run_build_gateway_log_reader() {
  if [ "${CT_BUILD_GATEWAY_FAIL_LOG_OPEN:-0}" = "1" ]; then
    printf 'ERROR: injected build gateway log open failure: %s\n' "$1" >&2
    return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = fixture ] && [ "${CT_BUILD_GATEWAY_STUCK_READER:-0}" = 1 ]; then
    exec 5<> "$CT_BUILD_GATEWAY_LOG_FIFO"
  fi
  exec tee -a "$1"
}

enter_build_gateway() {
  ct_gateway_acquire "$*"
  if [ "$CMD" != gc ]; then
    ct_cache_retention auto || echo "[cache-policy] retention refused; build continues" >&2
  fi
  local gateway_log
  gateway_log="$(ct_gateway_log_path)"
  echo "Build gateway job $CT_BUILD_JOB_ID log: $gateway_log"
  if [ "${CT_BUILD_GATEWAY_NO_TEE:-0}" = "1" ]; then
    echo "WARNING: build gateway log duplication is disabled by CT_BUILD_GATEWAY_NO_TEE=1" >&2
    return
  fi
  exec 3>&1 4>&2
  trap '' PIPE
  unset CT_BUILD_GATEWAY_LOG_READER_STATUS
  CT_BUILD_GATEWAY_LOG_FIFO="${gateway_log}.fifo.$$"
  if ! mkfifo "$CT_BUILD_GATEWAY_LOG_FIFO"; then
    printf 'ERROR: build gateway log channel could not be created: %s\n' \
      "$CT_BUILD_GATEWAY_LOG_FIFO" >&4
    exec 3>&- 4>&-
    ct_gateway_finish "$CT_BUILD_GATEWAY_LOGGING_FAILURE" "" logging_failed
    return "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  run_build_gateway_log_reader "$gateway_log" < "$CT_BUILD_GATEWAY_LOG_FIFO" >&3 &
  CT_BUILD_GATEWAY_LOG_READER_PID=$!
  exec > "$CT_BUILD_GATEWAY_LOG_FIFO" 2>&1
  CT_BUILD_GATEWAY_LOG_PATH="$gateway_log"
  CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN=0
  trap 'build_gateway_logging_on_exit' EXIT
  if [ "${CT_BUILD_GATEWAY_FAIL_LOG_OPEN:-0}" = "1" ]; then
    if wait "$CT_BUILD_GATEWAY_LOG_READER_PID"; then
      CT_BUILD_GATEWAY_LOG_READER_STATUS=0
    else
      CT_BUILD_GATEWAY_LOG_READER_STATUS=$?
    fi
    exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
  fi
  CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN=1
}

check_build_gateway_logging() {
  local root success_log handoff_log reader_success_status reader_failure_status open_status killed_status piped_status
  root="$(mktemp -d "${TMPDIR:-/tmp}/codeterm-gateway-logging.XXXXXX")"
  CT_BUILD_GATEWAY_CHECK_ROOT="$root"
  trap 'rm -rf -- "$CT_BUILD_GATEWAY_CHECK_ROOT"' EXIT
  CT_BUILD_JOB_ID=healthy CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" ./build.sh gateway-log-probe success \
    > "$root/success.console" 2>&1
  success_log="$(sed -n 's/^Build gateway job .* log: //p' "$root/success.console" | head -1 | tr -d '\r')"
  grep -q '^MARKER_LAST$' "$root/success.console" || {
    echo "build gateway logging regression: console lost the final marker" >&2
    return 1
  }
  grep -q '^MARKER_LAST$' "$success_log" || {
    echo "build gateway logging regression: gateway log lost the final marker" >&2
    return 1
  }
  echo "success: final marker reached console and gateway log"
  CT_BUILD_JOB_ID=handoff-success CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
    ./build.sh gateway-log-probe handoff-success > "$root/handoff.console" 2>&1
  handoff_log="$(sed -n 's/^Build gateway job .* log: //p' "$root/handoff.console" | head -1 | tr -d '\r')"
  grep -q '^HANDOFF_AFTER_GATEWAY$' "$root/handoff.console" || {
    echo "build gateway handoff regression: post-gateway handoff did not run" >&2
    return 1
  }
  if grep -q '^HANDOFF_AFTER_GATEWAY$' "$handoff_log"; then
    echo "build gateway handoff regression: installer handoff remained inside the build log" >&2
    return 1
  fi
  echo "handoff: gateway completed and logging closed before installer handoff"
  set +e
  CT_BUILD_JOB_ID=piped CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
    /bin/bash ./build.sh gateway-log-probe success 2>&1 | tail -n 20 > "$root/piped.console"
  piped_status=${PIPESTATUS[0]}
  set -e
  [ "$piped_status" -eq 0 ] || {
    echo "build gateway logging regression: piped Bash 3 invocation exited $piped_status" >&2
    return 1
  }
  grep -q '^MARKER_LAST$' "$root/piped.console" || {
    echo "build gateway logging regression: piped console lost the final marker" >&2
    return 1
  }
  echo "pipeline: Bash 3 retained the log reader child and command status"
  if CT_BUILD_JOB_ID=reader-success CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
      ./build.sh gateway-log-probe reader-failure-success > "$root/reader-success.console" 2>&1; then
    reader_success_status=0
  else
    reader_success_status=$?
  fi
  [ "$reader_success_status" -eq "$CT_BUILD_GATEWAY_LOGGING_FAILURE" ] || {
    echo "build gateway logging regression: reader failure after success exited $reader_success_status" >&2
    return 1
  }
  grep -q 'build gateway logging path broke.*command exit was 0' "$root/reader-success.console" || {
    echo "build gateway logging regression: reader failure was misclassified" >&2
    return 1
  }
  if CT_BUILD_JOB_ID=reader-failure CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
      ./build.sh gateway-log-probe reader-failure-command-fail > "$root/reader-failure.console" 2>&1; then
    reader_failure_status=0
  else
    reader_failure_status=$?
  fi
  [ "$reader_failure_status" -eq "$CT_BUILD_GATEWAY_LOGGING_FAILURE" ] || {
    echo "build gateway logging regression: reader failure after command failure exited $reader_failure_status" >&2
    return 1
  }
  grep -q 'build gateway logging path broke.*command exit was 101' "$root/reader-failure.console" || {
    echo "build gateway logging regression: failed command status was not reported" >&2
    return 1
  }
  echo "reader failures: exit 74 preserved command exits 0 and 101"
  if CT_BUILD_JOB_ID=open-failure CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
      ./build.sh gateway-log-probe open-failure \
      > "$root/open.console" 2>&1; then
    open_status=0
  else
    open_status=$?
  fi
  [ "$open_status" -eq "$CT_BUILD_GATEWAY_LOGGING_FAILURE" ] || {
    echo "build gateway logging regression: open failure exited $open_status" >&2
    return 1
  }
  grep -q 'build gateway logging path broke' "$root/open.console" || {
    echo "build gateway logging regression: open failure was misclassified" >&2
    return 1
  }
  grep -q 'command exit was UNKNOWN' "$root/open.console" || {
    echo "build gateway logging regression: open failure invented a command status" >&2
    return 1
  }
  if CT_BUILD_JOB_ID=command-killed CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" \
      ./build.sh gateway-log-probe command-killed > "$root/killed.console" 2>&1; then
    killed_status=0
  else
    killed_status=$?
  fi
  [ "$killed_status" -eq 130 ] || {
    echo "build gateway logging regression: killed command exited $killed_status" >&2
    return 1
  }
  grep -q 'Build gateway job command-killed exit: UNKNOWN' "$root/killed.console" || {
    echo "build gateway logging regression: killed command invented a command status" >&2
    return 1
  }
  local stuck_mode stuck_status
  for stuck_mode in success command-fail; do
    if CT_BUILD_JOB_ID="stuck-$stuck_mode" CT_BUILD_GATEWAY_READER_TIMEOUT_MS=250 \
        CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" ./build.sh gateway-log-probe "stuck-reader-$stuck_mode" \
        > "$root/stuck-$stuck_mode.console" 2>&1; then stuck_status=0; else stuck_status=$?; fi
    [ "$stuck_status" -eq "$CT_BUILD_GATEWAY_LOGGING_FAILURE" ] || return 1
    grep -q 'log reader did not reach EOF' "$root/stuck-$stuck_mode.console" || return 1
  done
  echo "stuck readers: EOF deadline released finished jobs with logging failure"
  CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$root/state" ./build.sh gateway-status > "$root/status.json"
  node - "$root/status.json" <<'JS'
const status = require(process.argv[2]);
const jobs = Object.fromEntries(status.recent.map((job) => [job.job_id, job]));
const expected = {
  healthy: ["success", 0, 0],
  piped: ["success", 0, 0],
  "handoff-success": ["success", 0, 0],
  "reader-success": ["logging_failed", 74, 0],
  "reader-failure": ["logging_failed", 74, 101],
  "open-failure": ["logging_failed", 74, null],
  "command-killed": ["cancelled", 130, null],
  "stuck-success": ["logging_failed", 74, 0],
  "stuck-command-fail": ["logging_failed", 74, 101],
};
for (const [id, values] of Object.entries(expected)) {
  const job = jobs[id];
  if (!job) throw new Error(`gateway status omitted ${id}`);
  const actual = [job.outcome, job.exit_code, job.command_exit_code];
  if (JSON.stringify(actual) !== JSON.stringify(values)) {
    throw new Error(`${id} recorded ${JSON.stringify(actual)}, expected ${JSON.stringify(values)}`);
  }
}
JS
  echo "job records: gateway outcome and command exit are independently recoverable"
  rm -rf -- "$root"
  trap - EXIT
  unset CT_BUILD_GATEWAY_CHECK_ROOT
  echo "build gateway logging regression: PASS"
}

prepare_exceptional_worktree_target() {
  local dir identity
  dir="$(ct_gateway_dir)"
  identity="$(printf '%s' "$(pwd -P)" | git hash-object --stdin)"
  export CARGO_TARGET_DIR="$(ct_exceptional_target_dir "$(pwd -P)" "$dir" "$identity")"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  # The marker is retention bookkeeping; a busy gateway mutex must not kill the build.
  if [ "$CARGO_TARGET_DIR" != "$(pwd -P)/target-check" ]; then
    ct_mark_exceptional_target || echo "[cache-policy] mark skipped; target stays protected while running"
  fi
  ct_verify_target_dir
  ct_require_prestarted_sccache
  echo "[cache-policy] exceptional worktree target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

prepare_release_target() {
  local dir
  dir="$(ct_gateway_dir)/targets/release/$CT_BUILD_JOB_ID"
  if [ -e "$dir" ]; then
    echo "ERROR: release target is not fresh: $dir" >&2
    exit 2
  fi
  export CARGO_TARGET_DIR="$dir"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  echo "[cache-policy] fresh release target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

prepare_local_install_target() {
  local dir canonical current identity
  current="$(git rev-parse --show-toplevel)"
  canonical="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
  if [ "$current" != "$canonical" ]; then
    echo "ERROR: install-local must run from the canonical checkout: $canonical" >&2
    exit 2
  fi
  identity="$(printf '%s' "$canonical" | git hash-object --stdin)"
  dir="$(ct_gateway_dir)/targets/local-install/$identity"
  node scripts/local-install-cache.mjs "$dir"
  export CARGO_TARGET_DIR="$dir"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  echo "[cache-policy] persistent local-install target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

parse_local_install_continuation() {
  LOCAL_INSTALL_CONTINUE_TAB="${CODETERM_TAB_ID:-}"
  LOCAL_INSTALL_CONTINUE_PROMPT=""
  LOCAL_INSTALL_CONTINUE_ENABLED=1

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --continue-prompt)
        [ "$#" -ge 2 ] || { echo "ERROR: --continue-prompt requires text" >&2; exit 2; }
        LOCAL_INSTALL_CONTINUE_PROMPT="$2"
        shift 2
        ;;
      --continue-tab)
        [ "$#" -ge 2 ] || { echo "ERROR: --continue-tab requires a Tab id" >&2; exit 2; }
        LOCAL_INSTALL_CONTINUE_TAB="$2"
        shift 2
        ;;
      --no-continue)
        LOCAL_INSTALL_CONTINUE_ENABLED=0
        shift
        ;;
      *)
        echo "ERROR: unknown install-local option: $1" >&2
        exit 2
        ;;
    esac
  done

  if [ "$LOCAL_INSTALL_CONTINUE_ENABLED" = "0" ]; then
    LOCAL_INSTALL_CONTINUE_TAB=""
    LOCAL_INSTALL_CONTINUE_PROMPT=""
    return
  fi

  if [ -z "$LOCAL_INSTALL_CONTINUE_TAB" ]; then
    if [ -n "$LOCAL_INSTALL_CONTINUE_PROMPT" ]; then
      echo "ERROR: install-local continuation needs --continue-tab or CODETERM_TAB_ID" >&2
      exit 2
    fi
    LOCAL_INSTALL_CONTINUE_ENABLED=0
    return
  fi

  if [ -z "$LOCAL_INSTALL_CONTINUE_PROMPT" ]; then
    local source_sha
    source_sha="$(git rev-parse HEAD)"
    LOCAL_INSTALL_CONTINUE_PROMPT="Local CodeTerm install completed for build $source_sha. Verify the production health endpoint reports this SHA, then continue the work that requested the reinstall."
  fi
}

require_release_source() {
  local actual expected dirty
  actual="$(git rev-parse HEAD)"
  expected="${CODETERM_EXPECTED_BUILD_SHA:-$actual}"
  if [ "$actual" != "$expected" ]; then
    echo "ERROR: release source mismatch: expected $expected, got $actual" >&2
    exit 2
  fi
  dirty="$(
    {
      git diff --name-only HEAD --
      git ls-files --others --exclude-standard
    } | sort -u
  )"
  if [ -n "$dirty" ]; then
    echo "ERROR: release source is not clean ($(printf '%s
' "$dirty" | wc -l | tr -d ' ') paths); refusing provenance capture" >&2
    printf '%s
' "$dirty" | head -n 20 >&2
    exit 2
  fi
}

# Copy the freshly-built CLI into a target bin dir. Development commands install
# only into their active dev data directory; production owns ~/.codeterm/bin.
cli_release_dir() {
  local target_root="${CARGO_TARGET_DIR:-cli/target}"
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "$target_root/$CARGO_BUILD_TARGET/release"
  else
    echo "$target_root/release"
  fi
}

cli_release_binary() {
  local release_dir
  release_dir="$(cli_release_dir)"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s/codeterm\n' "$release_dir"
  else
    printf '%s/codeterm.exe\n' "$release_dir"
  fi
}

cli_file_sha256() {
  local path="$1" output hash
  if command -v sha256sum >/dev/null 2>&1; then
    output="$(sha256sum -- "$path")" || return $?
    hash="${output%% *}"
  elif command -v shasum >/dev/null 2>&1; then
    output="$(shasum -a 256 -- "$path")" || return $?
    hash="${output%% *}"
  elif command -v openssl >/dev/null 2>&1; then
    output="$(openssl dgst -sha256 "$path")" || return $?
    hash="${output##* }"
  else
    return 127
  fi
  if [ "${#hash}" -ne 64 ] || [[ "$hash" == *[!0-9a-fA-F]* ]]; then
    return 1
  fi
  printf '%s\n' "$hash"
}

cli_file_sha256_or_status() {
  local path="$1" hash
  if [ ! -f "$path" ]; then
    printf '<missing>\n'
  elif hash="$(cli_file_sha256 "$path" 2>/dev/null)"; then
    printf '%s\n' "$hash"
  else
    printf '<unavailable>\n'
  fi
}

report_cli_install_failure() {
  local reason="$1" source="$2" destination="$3" source_hash="$4" destination_hash="$5"
  printf 'ERROR: CLI install failed: %s\n' "$reason" >&2
  printf '  built artifact: %s\n  built SHA-256: %s\n' "$source" "$source_hash" >&2
  printf '  installed path: %s\n  installed SHA-256: %s\n' "$destination" "$destination_hash" >&2
}

tauri_debug_dir() {
  local target_root="${CARGO_TARGET_DIR:-src-tauri/target}"
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "$target_root/$CARGO_BUILD_TARGET/debug"
  else
    echo "$target_root/debug"
  fi
}

dev_target_record() {
  printf '%s\n' "$CT_DATA_DIR/dev-target-debug"
}

record_dev_target() {
  local target="$1" target_win
  mkdir -p "$(dirname "$(dev_target_record)")"
  # A fresh named --wait target has only release/ until tauri dev links debug/.
  mkdir -p "$target"
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    target_win="$(cd "$target" 2>/dev/null && pwd -W 2>/dev/null || true)"
    [ -n "$target_win" ] || target_win="$(pwd -W 2>/dev/null || pwd)/$target"
    printf '%s\n' "$target_win" > "$(dev_target_record)"
  else
    (cd "$target" 2>/dev/null && pwd -P) > "$(dev_target_record)"
  fi
}

dev_app_path() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s\n' "$(tauri_debug_dir)/codeterm"
  else
    printf '%s\n' "$(tauri_debug_dir)/codeterm.exe"
  fi
}

dev_cli_path() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s\n' "$CT_DATA_DIR/bin/codeterm"
  else
    printf '%s\n' "$CT_DATA_DIR/bin/codeterm.exe"
  fi
}

# A dev PDB is overwritten by the very next link. The 2026-09-05 abort became
# unsymbolizable nine minutes after it was written, so the symbol file for each
# linked dev build is retained under <data dir>/pdb/<build sha>/ and the store is
# pruned to the newest few. The copy keeps the name codeterm.pdb because Windows
# symbol lookup finds a PDB by the name recorded in the binary's RSDS record — a
# renamed file is never found, which is why the sha is the directory.
DEV_PDB_KEEP_DEFAULT=3

dev_pdb_store() {
  printf '%s
' "${CODETERM_DEV_PDB_DIR:-$CT_DATA_DIR/pdb}"
}

dev_pdb_source() {
  printf '%s
' "${CARGO_TARGET_DIR:-$(pwd -P)/src-tauri/target}/debug/codeterm.pdb"
}

prune_dev_pdb_store() {
  local store keep stale dir
  store="$(dev_pdb_store)"
  [ -d "$store" ] || return 0
  keep="${CODETERM_DEV_PDB_KEEP:-$DEV_PDB_KEEP_DEFAULT}"
  stale="$(ls -1dt "$store"/*/ 2>/dev/null | tail -n +$((keep + 1)) || true)"
  [ -n "$stale" ] || return 0
  while IFS= read -r dir; do
    [ -n "$dir" ] || continue
    rm -rf "$dir"
    echo "[symbols] pruned $(basename "$dir")"
  done <<< "$stale"
}

# The symbol file's own debug identity, which is what the crash sidecar names.
# A commit sha cannot be the key: the same source relinks to a different symbol
# file every time, and a foreground retention runs after HEAD may have moved.
dev_pdb_key() {
  local src="$1" python_bin
  python_bin="$(command -v python3 || command -v python || true)"
  if [ -z "$python_bin" ]; then
    return 1
  fi
  "$python_bin" scripts/pdb_identity.py "$src" 2>/dev/null
}

retain_dev_pdb() {
  local sha="$1" src="${2:-$(dev_pdb_source)}" key="${3:-}" store dest
  if [ ! -f "$src" ]; then
    # Only the MSVC toolchain emits a PDB; elsewhere symbols live in the binary.
    return 0
  fi
  if [ -z "$key" ]; then
    key="$(dev_pdb_key "$src" || true)"
  fi
  if [ -z "$key" ]; then
    echo "[symbols] no debug identity in $src; symbol file not retained" >&2
    return 0
  fi
  store="$(dev_pdb_store)"
  dest="$store/$key"
  mkdir -p "$dest"
  if [ ! -f "$dest/codeterm.pdb" ] || [ "$src" -nt "$dest/codeterm.pdb" ]; then
    cp -f "$src" "$dest/codeterm.pdb.partial"
    mv -f "$dest/codeterm.pdb.partial" "$dest/codeterm.pdb"
    echo "[symbols] retained $key -> $dest/codeterm.pdb"
  else
    # Touch so an unchanged rerun still counts as the most recently used build.
    touch "$dest"
  fi
  # The sha is informative only — which source the symbols came from, for a human.
  printf '%s
' "${sha:-unknown}" > "$dest/build-sha.txt"
  prune_dev_pdb_store
}

dev_provenance_record() {
  printf '%s\n' "$CT_DATA_DIR/dev-build-provenance.json"
}

write_dev_provenance() {
  local build_sha="$1"
  node scripts/dev-prep.mjs --write-provenance \
    --record "$(dev_provenance_record)" \
    --build-sha "$build_sha" \
    --app "$(dev_app_path)" \
    --cli "$(dev_cli_path)" \
    --embed "$CT_DEV_EMBED_BUNDLE" \
    --frontend "$(pwd -P)/dist" \
    --instance "$CT_INSTANCE" \
    --api-port "$CT_PORT" \
    --vite-port "$CT_VITE_PORT" \
    --cdp-port "$CT_CDP_PORT" \
    --identifier "$CT_IDENTIFIER"
}

validate_dev_provenance() {
  node scripts/dev-prep.mjs --check-provenance \
    --record "$(dev_provenance_record)" \
    --app "$(dev_app_path)" \
    --cli "$(dev_cli_path)" \
    --frontend "$(pwd -P)/dist" \
    --instance "$CT_INSTANCE" \
    --api-port "$CT_PORT" \
    --vite-port "$CT_VITE_PORT" \
    --cdp-port "$CT_CDP_PORT" \
    --identifier "$CT_IDENTIFIER"
}

require_prebuilt_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "ERROR: prebuilt dev requires command '$command_name'" >&2
    return 1
  fi
}

preflight_dev_prebuilt_runtime() {
  local command_name pnpm_command="${CODETERM_TEST_PREBUILT_PNPM_COMMAND:-pnpm}"
  for command_name in node curl git sed head rm cat; do
    require_prebuilt_command "$command_name" || return
  done
  require_prebuilt_command "$pnpm_command" || return
  if ! "$pnpm_command" --version >/dev/null 2>&1; then
    echo "ERROR: prebuilt dev cannot execute pnpm" >&2
    return 1
  fi
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    for command_name in nohup lsof pgrep xargs; do
      require_prebuilt_command "$command_name" || return
    done
  else
    for command_name in netstat taskkill grep awk; do
      require_prebuilt_command "$command_name" || return
    done
    if ! command -v powershell.exe >/dev/null 2>&1 && ! command -v powershell >/dev/null 2>&1; then
      echo "ERROR: prebuilt dev requires PowerShell for owned-process cleanup" >&2
      return 1
    fi
  fi
  node scripts/dev-prep.mjs --check-runtime \
    --data-dir "$CT_DATA_DIR" \
    --log "$PREBUILT_LOG"
}

cleanup_failed_prebuilt_launch() {
  local status="${1:-$?}"
  trap - EXIT INT TERM
  echo "ERROR: prebuilt dev launch failed; cleaning the owned dev processes." >&2
  kill_dev || true
  [ "$status" -ne 0 ] || status=1
  exit "$status"
}

install_cli_to() {
  local bin_dir="$1"
  local source="${2:-}" expected_hash="${3:-}" destination source_hash destination_hash
  if [ -z "$source" ]; then
    source="$(cli_release_binary)"
  fi
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    destination="$bin_dir/codeterm"
  else
    destination="$bin_dir/codeterm.exe"
  fi

  source_hash="$(cli_file_sha256_or_status "$source")"
  if [ -z "$expected_hash" ]; then
    expected_hash="$source_hash"
  fi
  if [ "$expected_hash" = "<missing>" ] || [ "$expected_hash" = "<unavailable>" ]; then
    destination_hash="$(cli_file_sha256_or_status "$destination")"
    report_cli_install_failure "built artifact cannot be hashed" \
      "$source" "$destination" "$expected_hash" "$destination_hash"
    return 1
  fi
  if [ "$source_hash" != "$expected_hash" ]; then
    destination_hash="$(cli_file_sha256_or_status "$destination")"
    report_cli_install_failure "built artifact changed before copy (current SHA-256: $source_hash)" \
      "$source" "$destination" "$expected_hash" "$destination_hash"
    return 1
  fi
  if ! mkdir -p "$bin_dir" || ! cp "$source" "$destination"; then
    destination_hash="$(cli_file_sha256_or_status "$destination")"
    report_cli_install_failure "copy did not succeed" \
      "$source" "$destination" "$expected_hash" "$destination_hash"
    return 1
  fi

  destination_hash="$(cli_file_sha256_or_status "$destination")"
  if [ "$destination_hash" != "$expected_hash" ]; then
    report_cli_install_failure "installed binary does not match the built artifact" \
      "$source" "$destination" "$expected_hash" "$destination_hash"
    return 1
  fi
  echo "CLI installed to $bin_dir"
}

write_cli_docs() {
  local release_dir binary
  release_dir="$(cli_release_dir)"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    binary="$release_dir/codeterm"
  else
    binary="$release_dir/codeterm.exe"
  fi
  "$binary" cli-manifest --write
}

if [ "${CODETERM_TEST_PRINT_CLI_RELEASE_DIR:-}" = "1" ]; then
  cli_release_dir
  exit 0
fi

if [ -n "${CODETERM_TEST_INSTALL_CLI_TO:-}" ]; then
  install_cli_to "$CODETERM_TEST_INSTALL_CLI_TO" "${CODETERM_TEST_INSTALL_CLI_SOURCE:-}" \
    "${CODETERM_TEST_INSTALL_CLI_HASH:-}"
  exit $?
fi

if [ "${CODETERM_TEST_PRINT_BUILD_CACHE_POLICY:-}" = "1" ]; then
  ct_verify_target_dir
  printf 'identifier=%s\napi_port=%s\nvite_port=%s\ncdp_port=%s\ntauri_debug_dir=%s\n' \
    "$CT_IDENTIFIER" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$(tauri_debug_dir)"
  exit 0
fi

prepare_bundle_stubs() {
  node scripts/prepare-cli-stub.js
  node scripts/prepare-embed-stub.js
}

prepare_dev_frontend_dist() {
  node scripts/dev-resource-preflight.mjs "${1:-$(pwd -P)}"
}

# Reads changed paths on stdin; prints "app <package>" / "cli" lines, deduplicated.
check_queue_targets() {
  awk '
    /^src-tauri\/crates\/[^\/]+\// { split($0, p, "/"); print "app " p[3]; next }
    /^src-tauri\/(src|tests|benches|examples|build\.rs|Cargo\.(toml|lock))/ { print "app codeterm"; next }
    /^cli\// { print "cli"; next }
  ' | sort -u
}

# Reads cargo --message-format=short output on stdin; prints at most $1 diagnostics.
check_queue_errors() {
  awk -v max="$1" '
    /^[^ ].*:[0-9]+:[0-9]+: error(\[E[0-9]+\])?:/ || /^error(\[E[0-9]+\])?:/ && !/could not compile/ {
      total++
      if (total <= max) print
    }
    END { printf "errors=%d shown=%d\n", total, (total < max ? total : max) }
  '
}

# Reads cargo --message-format=short output on stdin; prints at most $1 unique
# warnings, one "warnings crate=<name> count=<n>" line per warned crate and a
# summary. Exits 1 when a crate named in $2 (space-separated; "*" = any) warned.
check_queue_warnings() {
  awk -v max="$1" -v touched=" $2 " '
    function add(name, n) {
      if (!(name in counts)) order[++crates] = name
      counts[name] += n
    }
    /^[^ ].*:[0-9]+:[0-9]+: warning:/ {
      if (!($0 in seen)) {
        seen[$0] = 1
        total++
        if (total <= max) print
      }
      next
    }
    /^(warning|error): (could not compile )?`[^`]+`.*([0-9]+ warnings? emitted|generated [0-9]+ warnings?)/ {
      match($0, /`[^`]+`/)
      name = substr($0, RSTART + 1, RLENGTH - 2)
      if (match($0, /generated [0-9]+/)) n = substr($0, RSTART + 10, RLENGTH - 10) + 0
      else { match($0, /[0-9]+ warnings? emitted/); n = substr($0, RSTART, RLENGTH) + 0 }
      if (match($0, /\([0-9]+ duplicates?\)/)) n -= substr($0, RSTART + 1, RLENGTH - 1) + 0
      add(name, n)
    }
    END {
      failing = ""
      for (i = 1; i <= crates; i++) {
        name = order[i]
        if (counts[name] <= 0) continue
        printf "warnings crate=%s count=%d\n", name, counts[name]
        if (touched == " * " || index(touched, " " name " ")) failing = failing (failing ? "," : "") name
      }
      printf "warnings=%d shown=%d failing=%s\n", total, (total < max ? total : max), (failing ? failing : "none")
      exit (failing ? 1 : 0)
    }
  '
}

# Reads cargo output in any format; prints one count line per warned crate and a total.
check_warning_summary() {
  check_queue_warnings 0 "" | awk '
    /^warnings crate=/ { print; split($3, kv, "="); total += kv[2]; crates++ }
    END { printf "check: warnings=%d crates_with_warnings=%d\n", total, crates }
  '
}

check_queue_worktree() {
  local dir
  dir="$(ct_gateway_dir)/check-worktree"
  if [ ! -e "$dir/.git" ]; then
    git worktree add -q --detach "$dir" "$1" >&2
  fi
  printf '%s\n' "$dir"
}

check_queue_run() {
  local sha="" max_errors=20 max_warnings=40 touched_only=0 all=0 arg base cw files targets pkg_args=() cli=0 started status=0 out touched=""
  while [ "$#" -gt 0 ]; do
    arg="$1"
    case "$arg" in
      --max-errors) max_errors="${2:?--max-errors requires a count}"; shift ;;
      --max-warnings) max_warnings="${2:?--max-warnings requires a count}"; shift ;;
      --touched-only) touched_only=1 ;;
      --all) all=1 ;;
      -*) echo "Usage: ./build.sh check-queue [<sha>] [--max-errors N] [--max-warnings N] [--touched-only] [--all]" >&2; exit 2 ;;
      *) sha="$arg" ;;
    esac
    shift
  done
  sha="$(git rev-parse --verify "${sha:-HEAD}^{commit}")"
  base="$(git merge-base "$sha" develop 2>/dev/null || git merge-base "$sha" origin/develop 2>/dev/null || true)"
  if [ "$all" = 1 ]; then
    files=""
    targets="all"
  elif [ -n "$base" ]; then
    files="$(git diff --name-only "$base" "$sha")"
    targets="$(printf '%s\n' "$files" | check_queue_targets)"
  else
    files="src-tauri/src/"
    targets="$(printf '%s\n' "$files" | check_queue_targets)"
  fi
  if [ -z "$targets" ]; then
    echo "check-queue: no Rust changes between ${base:-?} and $sha; nothing to check"
    return 0
  fi
  if [ "$all" = 1 ]; then
    pkg_args=(--workspace)
    cli=1
    touched="*"
  else
    while read -r kind pkg; do
      if [ "$kind" = cli ]; then cli=1; touched="${touched:-} codeterm-cli"; else pkg_args+=(-p "$pkg"); touched="${touched:-} $pkg"; fi
    done <<< "$targets"
  fi
  if [ "${#pkg_args[@]}" -gt 0 ] && [ "$all" = 0 ] && [ "$touched_only" = 0 ] && ! printf '%s\n' "${pkg_args[@]}" | grep -qx codeterm; then
    pkg_args+=(-p codeterm)
  fi
  source scripts/clippy-allow.sh

  CT_BUILD_SOURCE_SHA="$sha"
  CT_BUILD_CRATES="$(printf '%s\n' "${pkg_args[@]}" | sed '/^-p$/d')"
  [ "$cli" != 1 ] || CT_BUILD_CRATES="${CT_BUILD_CRATES}${CT_BUILD_CRATES:+ }codeterm-cli"
  CT_BUILD_TOUCHED_FILES_COUNT="$(printf '%s\n' "$files" | sed '/^$/d' | wc -l | tr -d ' ')"
  enter_build_gateway "$CMD $sha"
  cw="$(check_queue_worktree "$sha")"
  git -C "$cw" checkout -q --detach --force "$sha" >&2
  cd "$cw"
  prepare_dev_frontend_dist "$cw"
  prepare_bundle_stubs
  ct_configure_sccache
  unset CARGO_INCREMENTAL
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  started="$(date +%s)"
  out="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.check.log"
  : > "$out"
  if [ "${#pkg_args[@]}" -gt 0 ]; then
    CARGO_TARGET_DIR="$cw/target/app" cargo clippy --manifest-path src-tauri/Cargo.toml \
      "${pkg_args[@]}" --all-targets --no-deps --message-format=short -- "${CLIPPY_ALLOW[@]}" >> "$out" 2>&1 || status=$?
  fi
  if [ "$cli" = 1 ]; then
    CARGO_TARGET_DIR="$cw/target/cli" cargo clippy --manifest-path cli/Cargo.toml \
      --all-targets --no-deps --message-format=short -- "${CLIPPY_ALLOW[@]}" >> "$out" 2>&1 || status=$?
  fi
  grep 'cargo::core::compiler::fingerprint' "$out" || true
  check_queue_errors "$max_errors" < "$out"
  check_queue_warnings "$max_warnings" "$touched" < "$out" || [ "$status" != 0 ] || status=1
  echo "check-queue: sha=$sha packages=${pkg_args[*]:-} cli=$cli exit=$status wall=$(( $(date +%s) - started ))s full-log=$out"
  return "$status"
}

# Resolve the tauri dev config. For a named instance, generate a derived config
# whose devUrl points at this instance's frontend static port.
dev_conf_path() {
  local base="src-tauri/tauri.dev.conf.json"
  local out="src-tauri/tauri.dev.conf${CT_SUFFIX:--runtime}.json"
  node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('$base','utf8'));const defaults='--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection';const cdp='--remote-debugging-port=$CT_CDP_PORT';let args=process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS||cdp;if(!args.includes('--remote-debugging-port='))args=[args,cdp].filter(Boolean).join(' ');if(!args.includes('--disable-features='))args=[defaults,args].filter(Boolean).join(' ');c.identifier=process.env.CODETERM_DEV_IDENTIFIER;c.build=c.build||{};c.build.beforeDevCommand=null;c.build.devUrl='http://127.0.0.1:'+process.env.CODETERM_VITE_PORT;c.app=c.app||{};c.app.windows=c.app.windows&&c.app.windows.length?c.app.windows:[{}];c.app.windows[0]={...c.app.windows[0],additionalBrowserArgs:args};fs.writeFileSync('$out',JSON.stringify(c,null,2));" >&2
  echo "$out"
}

ensure_real_embed_bundle() {
  local host exe_ext embed_size
  host=$(rustc -vV | awk -F': ' '/^host:/{print $2}')
  exe_ext=""
  case "$OSTYPE" in msys*|cygwin*|win32) exe_ext=".exe" ;; esac
  CT_DEV_EMBED_BUNDLE="src-tauri/bundled-embed/codeterm-embed-${host}${exe_ext}"
  prepare_bundle_stubs
  embed_size=0
  if [ -f "$CT_DEV_EMBED_BUNDLE" ]; then
    embed_size=$(stat -c %s "$CT_DEV_EMBED_BUNDLE" 2>/dev/null || stat -f %z "$CT_DEV_EMBED_BUNDLE" 2>/dev/null || echo 0)
  fi
  if [ "${REBUILD_EMBED:-0}" = "1" ] || [ "$embed_size" -lt 1048576 ]; then
    echo "Building codeterm-embed before dev (size=${embed_size}, REBUILD_EMBED=${REBUILD_EMBED:-0})..."
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    CODETERM_DEV=1 node scripts/bundle-embed.js
  fi
}

start_dev_wait_process() {
  local log="$1"
  local conf
  conf="$(dev_conf_path)"
  local cmd=(pnpm tauri dev --no-watch --config "$conf")
  if [[ "$OSTYPE" == darwin* ]] && command -v screen >/dev/null 2>&1; then
    local session="codeterm-dev${CT_SUFFIX}-$$"
    local root_q log_q conf_q
    printf -v root_q "%q" "$(pwd)"
    printf -v log_q "%q" "$log"
    printf -v conf_q "%q" "$conf"
    screen -dmS "$session" bash -lc "cd $root_q && exec pnpm tauri dev --no-watch --config $conf_q >> $log_q 2>&1"
    echo "screen:$session"
    return
  fi
  local pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      "${cmd[@]}" </dev/null >> "$log" 2>&1 &
      ;;
    *)
      nohup "${cmd[@]}" </dev/null >> "$log" 2>&1 &
      ;;
  esac
  pid=$!
  spawn_status=$?
  set -e
  if [ -z "$pid" ]; then
    echo "Failed to spawn dev process (status=$spawn_status)" >&2
    return "${spawn_status:-1}"
  fi
  echo "$pid"
}

dev_wait_process_alive() {
  local pid="$1"
  case "$pid" in
    screen:*)
      local session="${pid#screen:}"
      screen -ls 2>/dev/null | grep -q "[.]$session[[:space:]]"
      ;;
    *)
      kill -0 "$pid" 2>/dev/null
      ;;
  esac
}

dev_wait_stage() {
  local log="$1" stage
  stage="$(tail -80 "$log" 2>/dev/null | sed '/^[[:space:]]*$/d' | tail -n1 || true)"
  if [ -n "$stage" ]; then
    printf '%s\n' "$stage"
  else
    printf '%s\n' 'launcher spawned; waiting for compiler or daemon output'
  fi
}

handoff_gateway_to_dev() {
  local dev_pid="$1" monitor_pid monitor_start lock
  lock="$(ct_gateway_dir)/lock"
  mkdir -p "$lock"
  (
    trap - EXIT INT TERM
    CT_GATEWAY_MUTEX_PID="$BASHPID"
    CT_GATEWAY_PROCESS_START="$(ct_gateway_process_start_token "$BASHPID")"
    sleep 1
    while dev_wait_process_alive "$dev_pid"; do
      sleep 2
    done
    CT_GATEWAY_ACTIVE=1
    ct_gateway_finish 0
  ) >/dev/null 2>&1 &
  monitor_pid=$!
  monitor_start="$(ct_gateway_process_start_token "$monitor_pid")"
  printf '%s\n' "$monitor_pid" > "$lock/pid"
  printf '%s\n' "$monitor_start" > "$lock/start_token"
  CT_GATEWAY_ACTIVE=0
  echo "Build gateway remains owned by dev monitor $monitor_pid until launcher $dev_pid exits."
}

# /web browser clients are served the static dist/ bundle, NOT vite HMR.
# Watch-rebuild it so phone/browser mirrors pick up frontend edits on reload.
web_watch_pidfile() {
  echo "/tmp/codeterm-web-watch${CT_SUFFIX}.pid"
}

dev_static_pidfile() {
  echo "/tmp/codeterm-dev-static${CT_SUFFIX}.pid"
}

kill_process_tree() {
  local pid="$1"
  if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
    return
  fi
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    local child
    for child in $(pgrep -P "$pid" 2>/dev/null || true); do
      kill_process_tree "$child"
    done
  else
    taskkill //PID "$pid" //T //F 2>/dev/null || true
    return
  fi
  kill "$pid" 2>/dev/null || true
}

managed_pid_from_file() {
  local pidfile="$1" pid
  [ -f "$pidfile" ] || return
  pid="$(cat "$pidfile" 2>/dev/null || true)"
  if [[ "$pid" =~ ^[0-9]+$ ]] && [ "$pid" -gt 0 ]; then
    echo "$pid"
  fi
}

process_pid_for_cleanup() {
  local shell_pid="$1" windows_pid
  case "$OSTYPE" in
    msys*|cygwin*)
      windows_pid="$(cat "/proc/$shell_pid/winpid" 2>/dev/null || true)"
      if [[ "$windows_pid" =~ ^[0-9]+$ ]] && [ "$windows_pid" -gt 0 ]; then
        echo "$windows_pid"
        return
      fi
      ;;
  esac
  echo "$shell_pid"
}

kill_orphan_web_watchers() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    return
  fi
  local root pid cwd
  root="$(pwd)"
  for pid in $(pgrep -f "vite build --watch" 2>/dev/null || true); do
    cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
    if [ "$cwd" = "$root" ]; then
      kill_process_tree "$pid"
    fi
  done
}

kill_orphan_dev_static_servers() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    return
  fi
  local root pid cwd
  root="$(pwd)"
  for pid in $(pgrep -f "scripts/dev-static-server.mjs" 2>/dev/null || true); do
    cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
    if [ "$cwd" = "$root" ]; then
      kill_process_tree "$pid"
    fi
  done
}

windows_dev_leftover_cleanup() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    return
  fi
  command -v powershell.exe >/dev/null 2>&1 || command -v powershell >/dev/null 2>&1 || return

  local root_win target_debug_win data_dir_win cleanup_script ps pid
  local -a owned_pids=() cleanup_args
  for pid in "$@"; do
    if [[ "$pid" =~ ^[0-9]+$ ]] && [ "$pid" -gt 0 ]; then
      owned_pids+=("$pid")
    fi
  done
  root_win="$(pwd -W 2>/dev/null || pwd)"
  target_debug_win="$(cat "$(dev_target_record)" 2>/dev/null || true)"
  if [ -z "$target_debug_win" ]; then
    target_debug_win="$(cd "$(tauri_debug_dir)" 2>/dev/null && pwd -W 2>/dev/null || echo "$(pwd -W 2>/dev/null || pwd)/$(tauri_debug_dir)")"
  fi
  data_dir_win="$(cd "$CT_DATA_DIR" 2>/dev/null && pwd -W 2>/dev/null || cygpath -w "$CT_DATA_DIR" 2>/dev/null || echo "$CT_DATA_DIR")"
  cleanup_script="$root_win/scripts/windows-dev-cleanup.ps1"
  if command -v powershell.exe >/dev/null 2>&1; then
    ps="powershell.exe"
  else
    ps="powershell"
  fi
  cleanup_args=(
    "$ps" -NoProfile -ExecutionPolicy Bypass -File "$cleanup_script" -Root "$root_win"
    -TargetDebug "$target_debug_win" -DataDir "$data_dir_win" -Identifier "$CT_IDENTIFIER" -Instance "$CT_INSTANCE"
    -VitePort "$CT_VITE_PORT" -ApiPort "$CT_PORT" -CdpPort "$CT_CDP_PORT"
  )
  if [ "${#owned_pids[@]}" -gt 0 ]; then
    cleanup_args+=(-OwnedProcessIds "${owned_pids[@]}" -OwnedOnly)
  fi
  "${cleanup_args[@]}" 2>/dev/null || true
}

start_web_watch() {
  kill_web_watch || true
  local log="/tmp/codeterm-web-watch${CT_SUFFIX}.log"
  local vite_entry="node_modules/vite/bin/vite.js"
  local pid shell_pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      CODETERM_FRONTEND_DEV=1 node "$vite_entry" build --watch </dev/null >> "$log" 2>&1 &
      ;;
    *)
      CODETERM_FRONTEND_DEV=1 nohup node "$vite_entry" build --watch </dev/null >> "$log" 2>&1 &
      ;;
  esac
  shell_pid=$!
  spawn_status=$?
  set -e
  if [ -z "$shell_pid" ] || [ "$spawn_status" -ne 0 ]; then
    echo "WARNING: web bundle watcher did not start (status=$spawn_status, log $log); continuing without dist/ auto-rebuilds." >&2
    return 0
  fi
  pid="$(process_pid_for_cleanup "$shell_pid")"
  if ! echo "$pid" > "$(web_watch_pidfile)"; then
    echo "WARNING: web bundle watcher pidfile was not written; continuing without managed watcher cleanup." >&2
    return 0
  fi
  echo "Web bundle watcher started (dist/ auto-rebuilds, pid $pid, log $log)"
}

start_dev_static_server() {
  kill_dev_static_server || true
  local log="/tmp/codeterm-dev-static${CT_SUFFIX}.log"
  local pid shell_pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      node scripts/dev-static-server.mjs </dev/null >> "$log" 2>&1 &
      ;;
    *)
      nohup node scripts/dev-static-server.mjs </dev/null >> "$log" 2>&1 &
      ;;
  esac
  shell_pid=$!
  spawn_status=$?
  set -e
  if [ -z "$shell_pid" ] || [ "$spawn_status" -ne 0 ]; then
    echo "ERROR: dev static server did not start (status=$spawn_status, log $log)" >&2
    return 1
  fi
  pid="$(process_pid_for_cleanup "$shell_pid")"
  echo "$pid" > "$(dev_static_pidfile)"
  echo "Dev static server started (devUrl port $CT_VITE_PORT, pid $pid, log $log)"
}

kill_web_watch() {
  local pidfile
  pidfile="$(web_watch_pidfile)"
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    if [ -f "$pidfile" ]; then
      windows_dev_leftover_cleanup "$(managed_pid_from_file "$pidfile")"
      rm -f "$pidfile"
    fi
  elif [ -f "$pidfile" ]; then
    kill_process_tree "$(managed_pid_from_file "$pidfile")"
    rm -f "$pidfile"
  fi
  kill_orphan_web_watchers
}

kill_dev_static_server() {
  local pidfile
  pidfile="$(dev_static_pidfile)"
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    if [ -f "$pidfile" ]; then
      windows_dev_leftover_cleanup "$(managed_pid_from_file "$pidfile")"
      rm -f "$pidfile"
    fi
  elif [ -f "$pidfile" ]; then
    kill_process_tree "$(managed_pid_from_file "$pidfile")"
    rm -f "$pidfile"
  fi
  kill_orphan_dev_static_servers
}

dev_health_status() {
  curl -s --max-time 2 "http://127.0.0.1:${CT_PORT}/api/health" 2>/dev/null || true
}

dev_frontend_ready() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    node scripts/dev-frontend-ready.mjs ready
    return
  fi
  curl -fs --max-time 3 "http://127.0.0.1:${CT_VITE_PORT}/" 2>/dev/null | grep -q "<div id=\"root\""
}

dev_frontend_snapshot() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    node scripts/dev-frontend-ready.mjs snapshot || true
    return
  fi
  if curl -fs --max-time 3 "http://127.0.0.1:${CT_VITE_PORT}/" >/tmp/codeterm-frontend-snapshot.html 2>/dev/null; then
    echo "frontend: vite returned HTML" >&2
  else
    echo "frontend: vite did not answer" >&2
  fi
}

dev_health_fields() {
  node -e '
    let input="";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => {
      const json=JSON.parse(input);
      const instance=json.instance ?? {};
      process.stdout.write([json.status ?? "", json.build_sha ?? "", instance.name ?? "", instance.api_port ?? "", instance.vite_port ?? "", instance.cdp_port ?? "", instance.identifier ?? ""].join("\n"));
    });
  '
}

validate_dev_health() {
  local expected_sha="$1" expected_instance="$2" expected_port="$3" expected_vite="$4" expected_cdp="$5" expected_identifier="$6" health="$7"
  local status sha instance port vite cdp identifier fields
  fields="$(printf '%s' "$health" | dev_health_fields 2>/dev/null || true)"
  status="$(printf '%s\n' "$fields" | sed -n '1p')"
  sha="$(printf '%s\n' "$fields" | sed -n '2p')"
  instance="$(printf '%s\n' "$fields" | sed -n '3p')"
  port="$(printf '%s\n' "$fields" | sed -n '4p')"
  vite="$(printf '%s\n' "$fields" | sed -n '5p')"
  cdp="$(printf '%s\n' "$fields" | sed -n '6p')"
  identifier="$(printf '%s\n' "$fields" | sed -n '7p')"
  if [ "$status" != "ok" ]; then
    echo "Health endpoint did not report status=ok" >&2
    return 1
  fi
  if [[ ! "$expected_sha" =~ ^[0-9a-f]{40}$ ]] || [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]] || [ "$sha" != "$expected_sha" ]; then
    echo "Health endpoint is stale or wrong-lane: expected build $expected_sha, got $sha" >&2
    return 1
  fi
  if [ "$instance" != "$expected_instance" ] || [ "$port" != "$expected_port" ] ||
      [ "$vite" != "$expected_vite" ] || [ "$cdp" != "$expected_cdp" ] || [ "$identifier" != "$expected_identifier" ]; then
    echo "Health endpoint is from the wrong instance: expected instance='$expected_instance' api='$expected_port' vite='$expected_vite' cdp='$expected_cdp' identifier='$expected_identifier', got instance='$instance' api='$port' vite='$vite' cdp='$cdp' identifier='$identifier'" >&2
    return 1
  fi
  printf '%s\n' "$sha"
}

dev_listener_pid() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null | head -1 || true
  else
    netstat -ano 2>/dev/null | grep ":$CT_PORT.*LISTEN" | awk '{print $5}' | head -1 || true
  fi
}

dev_notify_health_proof() {
  local tab_id="$1" health sha
  [ -n "${EXPECTED_BUILD_SHA:-}" ] || return 1
  health="$(dev_health_status)"
  sha="$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$health" 2>/dev/null)" || return 1
  dev_notify_ready_once "$DEV_NOTIFY_SENT_MARKER" "$tab_id" "$CT_INSTANCE" "$CT_PORT" "$sha" "$(dev_listener_pid)" "$(pwd -P)"
}

# Auto-install node deps if lockfile is newer than node_modules
ensure_deps() {
  if [ ! -d node_modules ] || [ pnpm-lock.yaml -nt node_modules/.package-lock.json ] 2>/dev/null; then
    echo "Installing dependencies..."
    CI=true pnpm install --frozen-lockfile 2>/dev/null || CI=true pnpm install
  fi
}

prepare_release_bundle_inputs() {
  local minimum_macos_version
  minimum_macos_version=$(node -p "require('./src-tauri/tauri.conf.json').bundle?.macOS?.minimumSystemVersion || ''")
  if [ -n "$minimum_macos_version" ]; then
    export MACOSX_DEPLOYMENT_TARGET="$minimum_macos_version"
  fi
  local cargo_target_args=()
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    cargo_target_args=(--target "$CARGO_BUILD_TARGET")
  fi
  pnpm build
  cargo build --release --timings --manifest-path cli/Cargo.toml "${cargo_target_args[@]}"
  node scripts/bundle-cli.js
  node scripts/prepare-embed-stub.js
  cargo build --release --timings --manifest-path src-tauri/Cargo.toml \
    "${cargo_target_args[@]}" \
    --features "${CODETERM_RELEASE_FEATURES:-embeddings},tauri/custom-protocol" \
    --bin codeterm --bin codeterm-embed
  node scripts/bundle-embed.js
}

case "$CMD" in
  help)
    cat <<'EOF'
CodeTerm build commands

  dev                         Start the visible shared dev instance
  dev-prebuilt                Restart a validated dev build without Cargo
  retain-pdb <sha> [pdb] [key]  Keep a linked dev build's symbol file (last 3)
  check                       Fast Rust compile check
  check-tests                 Compile app and CLI test profiles
  check-queue [sha]           Warm incremental cargo check (all targets) of a lane SHA in the per-machine check worktree
  runner-pause MACHINE        Park MACHINE-* GitHub runners (labels -> paused; unique labels lent to a same-OS runner)
  runner-resume MACHINE       Restore MACHINE-* runner labels and return lent labels
  runner-status               List runners with labels and paused machines
  frontend                    Production frontend typecheck + bundle
  release-inputs              Build target-aware frontend, CLI, and embed bundle inputs
  ci-archives                 Compile and archive test profiles with the shared cache
  worktree-check              Isolated lane-safe Rust compile check
  test-one <input-macos|input-windows|billing|account|analytics|app|frontend-bundle|plan|pty|pty-fork|util|browser|types|wire|sentinel|decision|provider-signals|storage|mem|mesh|plugin|cli|agent-detect|notification-core|repowatch|usage> NAME
                              Run one focused Rust test
  nextest <scope> [filterset]  Compile then run Rust tests through nextest
  test                        Run the application Rust unit suite
  verify                      Run app/CLI tests, TypeScript check, and source gates
  qa-prep                     Prepare a worktree for QA
  install-local               Build and install locally (cheap fmt/size/clean gates first)
  script-guards               Run the script catalogue gate and uncalled fixture guards
  qa-batch <label> '<cmd>'...  Run check first, then the commands, one QA receipt
  lane-recover keep|merge ...  Park a wrong-base lane on keep/<lane>, then merge it back

Long operations (check, check-tests, test-one, test, verify, build, install-local, release-preflight)
accept --notify-tab <tab> or BUILD_NOTIFY_TAB: one OK or early-failure receipt.
  build                       Build production artifacts
  cache-status                Show configured/server cache locations and endpoint health
  cache-verify                Verify shared S3 configuration and running server without changes
  cache-stats                 Show counters only from a live sccache server
  gateway-status              Show the machine build owner
  build-history [--last N] [--verb V] [--json]  Build durations, cache hits and rebuild reasons
  gc [--dry-run]              Reclaim expired build caches and sweep checkout targets

Gateway-backed commands fail fast when the slot is busy. Add --wait-build only
when blocking this terminal is intentional.
EOF
    ;;
  env)
    # Already loaded above. Use: source build.sh env
    ;;
  gc)
    if [ "$#" -gt 2 ] || { [ "$#" -eq 2 ] && [ "$2" != "--dry-run" ]; }; then
      echo "Usage: ./build.sh gc [--dry-run]" >&2
      exit 2
    fi
    enter_build_gateway "gc"
    ct_cache_retention gc "${2:-}"
    ;;
  fmt-check)
    cargo fmt --manifest-path src-tauri/Cargo.toml --check
    cargo fmt --manifest-path cli/Cargo.toml --check
    ;;
  cache-status)
    ct_sccache_status
    ;;

  cache-verify)
    ct_sccache_verify
    ;;
  cache-stats)
    ct_sccache_stats
    ;;
  gateway-status)
    ct_gateway_status
    ;;
  build-history)
    node scripts/build-history.mjs "${@:2}" --dir "$(ct_gateway_dir)" \
      --canonical "$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
    ;;
  gateway-cancel)
    ct_gateway_cancel "${2:?gateway-cancel requires a job id}"
    ;;
  gateway-health-check)
    validate_dev_health "${2:?expected SHA required}" "${3:-}" "${4:?expected port required}" \
      "${5:?expected vite port required}" "${6:?expected cdp port required}" "${7:?expected identifier required}" \
      "$(cat "${8:?health fixture path required}")"
    ;;
  gateway-fake|gateway-sleep)
    enter_build_gateway "$CMD ${*:2}"
    fake_name="${2:-fake}"
    fake_duration="${3:-${2:-1}}"
    if [ -n "${CODETERM_GATEWAY_FAKE_EVENTS:-}" ]; then
      printf '%s start %s\n' "$fake_name" "$(node -p '(BigInt(Date.now()) * 1000000n).toString()')" >> "$CODETERM_GATEWAY_FAKE_EVENTS"
    fi
    if [[ "$fake_duration" == wait:* ]]; then
      fake_release="${fake_duration#wait:}"
      fake_deadline=$(( $(date +%s) + ${CODETERM_GATEWAY_FAKE_WAIT_SECS:-30} ))
      while [ ! -e "$fake_release" ]; do
        [ "$(date +%s)" -le "$fake_deadline" ] || { echo "Fake gateway release timed out: $fake_release" >&2; exit 124; }
        sleep 0.05
      done
    else
      sleep "$fake_duration"
    fi
    if [ -n "${CODETERM_GATEWAY_FAKE_EVENTS:-}" ]; then
      printf '%s end %s\n' "$fake_name" "$(node -p '(BigInt(Date.now()) * 1000000n).toString()')" >> "$CODETERM_GATEWAY_FAKE_EVENTS"
    fi
    [ "${4:-success}" != "fail" ] || exit 23
    ;;
  gateway-log-probe)
    log_probe_mode="${2:-success}"
    [ "$log_probe_mode" != "open-failure" ] || CT_BUILD_GATEWAY_FAIL_LOG_OPEN=1
    case "$log_probe_mode" in stuck-reader-*) CT_BUILD_GATEWAY_STUCK_READER=1 ;; esac
    enter_build_gateway "$CMD $log_probe_mode"
    printf 'MARKER_FIRST\n'
    case "$log_probe_mode" in
      reader-failure-success)
        break_build_gateway_log_reader
        exit 0
        ;;
      reader-failure-command-fail)
        break_build_gateway_log_reader
        exit 101
        ;;
      stuck-reader-command-fail)
        exit 101
        ;;
      command-killed)
        kill -TERM "$$"
        ;;
      handoff-success)
        complete_build_gateway_before_install_handoff
        printf 'HANDOFF_AFTER_GATEWAY\n'
        ;;
    esac
    printf 'MARKER_LAST\n'
    ;;
  gateway-logging-check)
    check_build_gateway_logging
    ;;
  gateway-policy-probe)
    case "${2:-}" in
      release-source) require_release_source ;;
      release-target)
        enter_build_gateway "$CMD ${*:2}"
        prepare_release_target
        printf 'target=%s\nincremental=%s\n' "$CARGO_TARGET_DIR" "$CARGO_INCREMENTAL"
        ;;
      worktree-target)
        enter_build_gateway "$CMD ${*:2}"
        prepare_exceptional_worktree_target
        printf 'target=%s\nincremental=%s\nwrapper=%s\n' "$CARGO_TARGET_DIR" "$CARGO_INCREMENTAL" "$RUSTC_WRAPPER"
        ;;
      qa-child)
        enter_build_gateway "qa-prep"
        bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "$$" "$CT_GATEWAY_PROCESS_START" "qa-prep" --probe-only
        ;;
      dev-resources)
        enter_build_gateway "dev-resource-preflight"
        prepare_dev_frontend_dist "${3:?dev-resources requires a fixture root}"
        ;;
      *) echo "Unknown gateway policy probe: ${2:-}" >&2; exit 2 ;;
    esac
    ;;
  retain-pdb)
    # ./build.sh retain-pdb <build-sha> [pdb-path] [key] — the seam the dev paths call
    # and scripts/check-build-pdb-retention.sh drives against a fixture store.
    retain_dev_pdb "${2:-}" "${3:-$(dev_pdb_source)}" "${4:-}"
    ;;
  check)
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    check_log="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.cargo-check.log"
    cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tee "$check_log"
    check_status="${PIPESTATUS[0]}"
    check_warning_summary < "$check_log"
    exit "$check_status"
    ;;
  check-queue)
    check_queue_run "${@:2}"
    ;;
  runner-pause)
    node scripts/runner-toggle.mjs pause "${2:?runner-pause requires a machine, e.g. BLACKSTAR}"
    ;;
  runner-resume)
    node scripts/runner-toggle.mjs resume "${2:?runner-resume requires a machine, e.g. BLACKSTAR}"
    ;;
  runner-status)
    node scripts/runner-toggle.mjs status
    ;;
  check-queue-probe)
    case "${2:-}" in
      targets) check_queue_targets ;;
      errors) check_queue_errors "${3:?errors requires a max}" ;;
      warnings) check_queue_warnings "${3:?warnings requires a max}" "${4:-}" ;;
      summary) check_warning_summary ;;
      *) echo "Unknown check-queue probe: ${2:-}" >&2; exit 2 ;;
    esac
    ;;
  check-tests)
    ct_configure_sccache
    prepare_exceptional_worktree_target
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --no-run
    cargo test --manifest-path cli/Cargo.toml --all-targets --no-run
    ;;
  frontend)
    enter_build_gateway "$CMD ${*:2}"
    ensure_deps
    pnpm build
    ;;
  ci-archives)
    ct_configure_sccache
    node scripts/collect-rust-warnings.mjs run --log "${RUST_WARNING_LOG:-ci-rust-warnings.log}" -- node scripts/ci-rust-archives.mjs
    ;;
  release-inputs)
    ct_configure_sccache
    ensure_deps
    prepare_release_bundle_inputs
    ;;
  release-preflight)
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ensure_deps
    ct_configure_sccache
    cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
    cargo fmt --manifest-path cli/Cargo.toml --all --check
    bash scripts/check-file-sizes.sh --fail
    node scripts/check-provider-boundary.mjs
    bash scripts/check-empty-array-expansion.sh
    bash scripts/check-script-catalog.sh
    node scripts/check-plugin-bundles.mjs
    node scripts/sync-plugin-versions.mjs --check
    cargo check --manifest-path src-tauri/Cargo.toml
    cargo check --manifest-path cli/Cargo.toml
    node scripts/check-release-locks.mjs
    bash scripts/clippy-gate.sh
    run_workspace_lib_tests
    scripts/test-capped.sh --manifest-path cli/Cargo.toml
    node node_modules/typescript/bin/tsc --noEmit
    node node_modules/knip/bin/knip.js --workspace @codeterm/chat-core --exports
    node scripts/check-api-effect-dependencies.mjs
    node scripts/run-ts-unit-tests.mjs
    node node_modules/vitest/vitest.mjs run
    node scripts/run-plugin-tests.mjs
    "$(command -v python3 || command -v python)" -m unittest scripts.tests.test_release_issue_contract
    "$(command -v python3 || command -v python)" -m unittest scripts.tests.test_release_workflow_contract
    node --test scripts/tests/updater-manifest.test.mjs
    node --test scripts/tests/release-closure-authority.test.mjs
    node --test scripts/tests/release-notes-sync.test.mjs
    node --test scripts/tests/bundled-cli-name.test.mjs
    node --test scripts/tests/local-install-cache.test.mjs
    node node_modules/@biomejs/biome/bin/biome check .
    ;;
  test)
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    run_workspace_lib_tests
    ;;
  test-one)
    test_scope="${2:?test-one requires billing, account, analytics, app, frontend-bundle, plan, pty, pty-fork, util, browser, types, wire, sentinel, decision, provider-signals, storage, mem, mesh, plugin, plugins, agent-detect, notification-core, repowatch, usage, or cli}"
    test_filter="${3:?test-one requires a test name or filter}"
    case "$test_scope" in
      app)
        enter_build_gateway "$CMD ${*:2}"
        if [ "$test_filter" = "build_identity_required" ]; then
          scripts/test-capped.sh --exec node --test scripts/tests/build_identity_required.test.mjs
          exit $?
        fi
        prepare_dev_frontend_dist "$(pwd -P)"
        prepare_bundle_stubs
        prepare_exceptional_worktree_target
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm --lib "$test_filter"
        ;;
      input-macos|input-windows)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p "codeterm-$test_scope" --lib "$test_filter"
        ;;
      frontend-bundle|codeterm-frontend-bundle)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-frontend-bundle --lib "$test_filter"
        ;;
      agent-detect|codeterm-agent-detect)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-agent-detect --lib "$test_filter"
        ;;
      billing) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-billing/Cargo.toml -p codeterm-billing --lib "$test_filter" ;;
      usage)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-usage/Cargo.toml -p codeterm-usage --lib "$test_filter"
        ;;
      account) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-account/Cargo.toml -p codeterm-account --lib "$test_filter" ;;
      notification-core|codeterm-notification-core)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-notification-core --lib "$test_filter"
        ;;
      analytics) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-analytics/Cargo.toml -p codeterm-analytics --lib "$test_filter" ;;
      repowatch|codeterm-repowatch)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-repowatch --lib "$test_filter"
        ;;
      plan) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-plan/Cargo.toml -p codeterm-plan --lib "$test_filter" ;;
      pty) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-pty/Cargo.toml -p codeterm-pty "$test_filter" ;;
      # pty-fork is excluded from the workspace (its own Cargo.lock) but owns
      # the conpty test-isolation boundary (win/psuedocon.rs). Kept as its own
      # scope rather than chained onto `pty` so a pre-existing standalone-build
      # issue in that vendored, previously-untested crate can't newly break
      # the working codeterm-pty test path.
      pty-fork) scripts/test-capped.sh --manifest-path src-tauri/pty-fork/Cargo.toml "$test_filter" ;;
      util|codeterm-util)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-util --lib "$test_filter"
        ;;
      browser|codeterm-browser)
        enter_build_gateway "$CMD ${*:2}"
        scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-browser --lib "$test_filter"
        ;;
      types) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-types/Cargo.toml -p codeterm-types --all-targets "$test_filter" ;;
      wire) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-wire/Cargo.toml -p codeterm-wire --all-targets "$test_filter" ;;
      sentinel) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-sentinel/Cargo.toml -p codeterm-sentinel --all-targets "$test_filter" ;;
      decision) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-decision/Cargo.toml -p codeterm-decision --all-targets "$test_filter" ;;
      provider-signals) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-provider-signals/Cargo.toml -p codeterm-provider-signals --all-targets "$test_filter" ;;
      storage) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-storage/Cargo.toml -p codeterm-storage --lib "$test_filter" ;;
      mem) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-mem/Cargo.toml -p codeterm-mem --lib "$test_filter" ;;
      mesh) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-mesh/Cargo.toml -p codeterm-mesh --lib "$test_filter" ;;
      blueprint-types|codeterm-blueprint-types) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-blueprint-types/Cargo.toml -p codeterm-blueprint-types --lib "$test_filter" ;;
      local-recording|codeterm-local-recording) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-local-recording/Cargo.toml -p codeterm-local-recording --lib "$test_filter" ;;
      plugin) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-plugin/Cargo.toml -p codeterm-plugin --lib "$test_filter" ;;
      # Plural crate (channel/install/update), distinct from the singular confinement crate above.
      plugins) scripts/test-capped.sh --manifest-path src-tauri/crates/codeterm-plugins/Cargo.toml -p codeterm-plugins --lib "$test_filter" ;;
      cli) scripts/test-capped.sh --manifest-path cli/Cargo.toml --all-targets "$test_filter" ;;
      *) echo "Unknown test scope: $test_scope (expected billing, account, analytics, app, frontend-bundle, plan, pty, pty-fork, util, browser, types, wire, sentinel, decision, provider-signals, storage, mem, mesh, plugin, plugins, agent-detect, notification-core, repowatch, usage, or cli)" >&2; exit 2 ;;
    esac
    ;;
  nextest)
    run_nextest_scope "${@:2}"
    ;;
  verify)
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ensure_deps
    ct_configure_sccache
    cargo check --manifest-path src-tauri/Cargo.toml
    run_workspace_lib_tests
    TEST_CAP_SECS="${TEST_CAP_SECS:-3600}" scripts/test-capped.sh --manifest-path cli/Cargo.toml
    node node_modules/typescript/bin/tsc --noEmit
    bash scripts/check-file-sizes.sh --fail
    node scripts/check-provider-boundary.mjs
    bash scripts/check-empty-array-expansion.sh
    bash scripts/check-script-catalog.sh
    ;;
  worktree-check)
    prepare_exceptional_worktree_target
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    cargo check --manifest-path src-tauri/Cargo.toml --all-targets "${@:2}"
    ;;
  build)
    enter_build_gateway "$CMD ${*:2}"
    node scripts/check-release-locks.mjs
    require_release_source
    ct_load_release_signing_key
    prepare_release_target
    ensure_deps
    ct_configure_sccache
    prepare_release_bundle_inputs
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    pnpm tauri build
    ct_check_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    # Release gate: /web is served from the exe-adjacent dist/ bundle that
    # tauri.conf bundle.resources maps in (../dist -> dist). If Tauri's
    # The release input step produced no dist/index.html, so /web would ship broken.
    if [ ! -f dist/index.html ]; then
      echo "❌ RELEASE GATE: dist/index.html missing after build — /web would 404. Run 'pnpm build'." >&2
      exit 1
    fi
    echo "✅ web bundle present (dist/index.html) — packaged into installer via bundle.resources"
    ;;
  install-local)
    parse_local_install_continuation "${@:2}"
    if [ "${CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION:-0}" = "1" ]; then
      printf 'enabled=%s\ntab=%s\nprompt=%s\n' \
        "$LOCAL_INSTALL_CONTINUE_ENABLED" \
        "$LOCAL_INSTALL_CONTINUE_TAB" \
        "$LOCAL_INSTALL_CONTINUE_PROMPT"
      exit 0
    fi
    case "$OSTYPE" in
      msys*|cygwin*|win32*) ;;
      *)
        echo "ERROR: install-local is supported only on Windows" >&2
        exit 2
        ;;
    esac
    cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
    cargo fmt --manifest-path cli/Cargo.toml --all --check
    bash scripts/check-file-sizes.sh --fail
    require_release_source
    enter_build_gateway "$CMD ${*:2}"
    node scripts/check-release-locks.mjs
    ct_load_release_signing_key
    prepare_local_install_target
    ensure_deps
    ct_configure_sccache
    prepare_release_bundle_inputs
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    rm -rf "$CARGO_TARGET_DIR/release/bundle/nsis"
    pnpm tauri build
    ct_check_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    VERSION="$(node -p "require('./package.json').version")"
    INSTALLER="$CARGO_TARGET_DIR/release/bundle/nsis/CodeTerm_${VERSION}_x64-setup.exe"
    if [ ! -f "$INSTALLER" ]; then
      echo "ERROR: local installer missing after build: $INSTALLER" >&2
      exit 1
    fi
    # Whatever bundle-cli.js staged — the name carries the host triple, so do not
    # second-guess it here.
    FRESH_CLI="$(ls src-tauri/bundled-cli/codeterm*.exe 2>/dev/null | head -1)"
    if [ ! -x "$FRESH_CLI" ]; then
      echo "ERROR: freshly bundled CLI missing after build: $FRESH_CLI" >&2
      exit 1
    fi
    echo "Local installer ready: $INSTALLER"
    echo "Installer handoff CLI: $FRESH_CLI"
    INSTALL_ARGS=(update install --from-file "$(cygpath -w "$INSTALLER")")
    if [ "$LOCAL_INSTALL_CONTINUE_ENABLED" = "1" ]; then
      INSTALL_ARGS+=(--continue-prompt "$LOCAL_INSTALL_CONTINUE_PROMPT" --continue-tab "$LOCAL_INSTALL_CONTINUE_TAB")
      echo "Post-install continuation: Tab $LOCAL_INSTALL_CONTINUE_TAB"
    else
      echo "Post-install continuation: disabled (no calling CodeTerm Tab)"
      INSTALL_ARGS+=(--no-continue)
    fi
    complete_build_gateway_before_install_handoff
    echo "Build gateway released; starting detached installer handoff."
    env \
      -u CODETERM_INSTANCE \
      -u CODETERM_VITE_PORT \
      -u CODETERM_DEV_IDENTIFIER \
      -u CODETERM_CDP_PORT \
      -u CODETERM_ALLOW_EXTERNAL_TARGET \
      -u CARGO_TARGET_DIR \
      -u CARGO_INCREMENTAL \
      -u RUSTC_WRAPPER \
      -u SCCACHE_BASEDIRS \
      "$FRESH_CLI" "${INSTALL_ARGS[@]}"
    ;;
  script-guards)
    # Cheap fixture guards for build/dev/QA scripts; Linux-only ones need a POSIX-clean runner (CI linux-ci).
    sg_failed=0
    sg_run() { echo "== $*"; "$@" || { echo "FAILED: $*" >&2; sg_failed=1; }; }
    sg_run bash scripts/check-script-catalog.sh
    for sg in check-build-check-queue.sh check-build-isolated-dev-policy.sh check-dev-resource-preflight.sh check-health-identity.sh check-local-install-continuation.sh       check-no-raw-graph-sql.sh check-visual-golden-edits.sh test-clippy-gate.sh test-descendant-cleanup.sh test-merge-stack-audit.sh test-timeout-seam.sh; do
      sg_run bash "scripts/$sg"
    done
    if [ "$(uname -s)" = Linux ]; then
      sg_run bash scripts/check-build-dev-port-overrides.sh
      sg_run bash scripts/check-cache-provenance.sh
      sg_run node --test scripts/check-analytics-catalog.test.mjs
    fi
    exit "$sg_failed"
    ;;
  qa-batch)
    # ./build.sh qa-batch <label> [--notify-tab <tab>] '<cmd>' ... — `check` always runs first.
    qa_label="${2:?qa-batch requires a label then one or more quoted commands}"
    qa_cmds=()
    qa_tab="${BUILD_NOTIFY_TAB:-}"
    shift 2
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --notify-tab) qa_tab="${2:?--notify-tab requires a non-empty tab id}"; shift 2 ;;
        --notify-tab=*) qa_tab="${1#--notify-tab=}"; shift ;;
        *) qa_cmds+=("$1"); shift ;;
      esac
    done
    [ "${#qa_cmds[@]}" -gt 0 ] || { echo "ERROR: qa-batch requires at least one command after the label" >&2; exit 2; }
    QA_NOTIFY_TAB="$qa_tab" bash scripts/qa-report.sh "$qa_label" "./build.sh check" "${qa_cmds[@]}"
    ;;
  lane-recover)
    # ./build.sh lane-recover keep <lane-branch> <worktree> — park the tip on keep/<lane-branch>, drop the clean worktree and lane branch.
    # ./build.sh lane-recover merge <lane-branch>           — real-merge keep/<lane-branch> into the current branch.
    lr_mode="${2:?lane-recover requires keep or merge}"
    lr_branch="${3:?lane-recover requires the lane branch name}"
    case "$lr_mode" in
      keep)
        lr_tree="${4:?lane-recover keep requires the worktree path}"
        [ -z "$(git -C "$lr_tree" status --porcelain)" ] || { echo "ERROR: worktree $lr_tree is dirty; commit or inspect it first" >&2; exit 2; }
        git rev-parse --verify --quiet "refs/heads/$lr_branch" >/dev/null || { echo "ERROR: no branch $lr_branch" >&2; exit 2; }
        git branch --force "keep/$lr_branch" "$lr_branch"
        git worktree remove "$lr_tree"
        git branch -D "$lr_branch"
        echo "Kept tip on keep/$lr_branch; re-dispatch the lane with the correct base, then: ./build.sh lane-recover merge $lr_branch"
        ;;
      merge)
        git merge --no-ff -m "Merge kept lane $lr_branch" "keep/$lr_branch"
        git merge-base --is-ancestor "keep/$lr_branch" HEAD && git branch -d "keep/$lr_branch"
        ;;
      *) echo "ERROR: lane-recover mode must be keep or merge" >&2; exit 2 ;;
    esac
    ;;
  qa-prep)
    enter_build_gateway "$CMD ${*:2}"
    bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "$$" "$CT_GATEWAY_PROCESS_START" "$CMD ${*:2}" -- "${@:2}"
    ;;
  bootstrap)
    enter_build_gateway "$CMD ${*:2}"
    ensure_deps
    prepare_bundle_stubs
    ct_configure_sccache
    cargo build --release --manifest-path cli/Cargo.toml
    CODETERM_DEV=1 node scripts/bundle-cli.js
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    CODETERM_DEV=1 node scripts/bundle-embed.js
    echo "Bootstrap complete: bundled CLI and codeterm-embed are present."
    ;;
  embed)
    enter_build_gateway "$CMD ${*:2}"
    # Force-rebuild the codeterm-embed binary. Skipped on every dev cycle
    # by default — use this when you change embed sources or pull new
    # ONNX/embeddings deps.
    prepare_bundle_stubs
    ct_configure_sccache
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    CODETERM_DEV=1 node scripts/bundle-embed.js
    echo "codeterm-embed rebuilt"
    ;;
  cli)
    echo "[build] CLI uses Cargo's cli/target lock; app/dev gateway is independent."
    ct_configure_sccache
    cargo build --release --manifest-path cli/Cargo.toml
    built_cli_artifact="$(cli_release_binary)"
    built_cli_hash="$(cli_file_sha256_or_status "$built_cli_artifact")"
    node scripts/bundle-cli.js
    write_cli_docs
    install_cli_to "$CT_DATA_DIR/bin" "$built_cli_artifact" "$built_cli_hash"
    ;;
  clean)
    enter_build_gateway "$CMD ${*:2}"
    cargo clean --manifest-path src-tauri/Cargo.toml
    ;;
  kill)
    kill_dev
    echo "Done."
    ;;
  dev-prebuilt)
    if [ -n "$CT_INSTANCE" ] && [ -z "${CARGO_TARGET_DIR:-}" ]; then
      export CARGO_TARGET_DIR="$(named_dev_target_dir)"
      export CODETERM_ALLOW_EXTERNAL_TARGET=1
    fi
    PREBUILT_LOG="/tmp/codeterm-dev-prebuilt${CT_SUFFIX}.log"
    if [ -n "$DEV_NOTIFY_TAB" ]; then
      : > "$PREBUILT_LOG"
      DEV_NOTIFY_LOG="$PREBUILT_LOG"
    fi
    DEV_NOTIFY_STAGE=preflight
    if ! validate_dev_provenance; then
      echo "ERROR: prebuilt dev provenance is absent, stale, or unparseable; refusing before stopping dev." >&2
      echo "Run ./build.sh dev --wait to rebuild and record valid dev provenance." >&2
      exit 2
    fi
    if ! preflight_dev_prebuilt_runtime; then
      echo "ERROR: prebuilt dev runtime prerequisites are unavailable; refusing before stopping dev." >&2
      echo "Run ./build.sh dev --wait to restore prerequisites and record valid dev provenance." >&2
      exit 2
    fi
    kill_dev
    if [ -n "$DEV_NOTIFY_TAB" ]; then
      trap 'status=$?; dev_notify_exit_report "$status"; cleanup_failed_prebuilt_launch "$status"' EXIT
      trap 'dev_notify_exit_report 130; cleanup_failed_prebuilt_launch 130' INT
      trap 'dev_notify_exit_report 143; cleanup_failed_prebuilt_launch 143' TERM
    else
      trap cleanup_failed_prebuilt_launch EXIT INT TERM
    fi
    export PATH="$CT_DATA_DIR/bin:$PATH"
    export CODETERM_CLI_PATH="$(dev_cli_path)"
    export CODETERM_DEV=1
    export CODETERM_WEB_DIST="$(pwd -P)/dist"
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=$CT_CDP_PORT}"
    node scripts/dev-prep.mjs --no-server
    start_web_watch
    start_dev_static_server
    if [ "$CT_DEV_WAIT" -eq 1 ]; then
      DEV_NOTIFY_STAGE=launch
      node scripts/dev-prep.mjs --launch-prebuilt \
        --app "$(dev_app_path)" --log "$PREBUILT_LOG" --api-port "$CT_PORT"
      HEALTH="$(dev_health_status)"
      EXPECTED_BUILD_SHA="$(git rev-parse HEAD)"
      SHA="$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH")"
      echo "Prebuilt dev ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', listener pid $(dev_listener_pid)"
      if [ -n "$DEV_NOTIFY_TAB" ]; then
        dev_notify_ready_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CT_INSTANCE" "$CT_PORT" "$SHA" "$(dev_listener_pid)" "$(pwd -P)"
        dev_notify_exit_report 0
      fi
      trap - EXIT INT TERM
    else
      echo "Starting provenance-validated prebuilt dev..."
      if [ -n "$DEV_NOTIFY_TAB" ]; then
        INTERACTIVE_LOG="/tmp/codeterm-dev${CT_SUFFIX}.interactive.log"
        : > "$INTERACTIVE_LOG"
        DEV_NOTIFY_LOG="$INTERACTIVE_LOG"
        EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
        DEV_NOTIFY_STAGE=launch
        DEV_NOTIFY_WAITER_PID=
        exec 3> >(tee -a "$INTERACTIVE_LOG" | report_read_stream "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CMD" launch "$INTERACTIVE_LOG" dev)
        DEV_NOTIFY_TEE_PID=$!
        if ( exec "$(dev_app_path)" ) >&3 2>&1; then
          DEV_NOTIFY_STATUS=0
        else
          DEV_NOTIFY_STATUS=$?
        fi
        exec 3>&-
        DEV_NOTIFY_TEE_STATUS=0
        if wait "$DEV_NOTIFY_TEE_PID"; then
          DEV_NOTIFY_TEE_STATUS=0
        else
          DEV_NOTIFY_TEE_STATUS=$?
        fi
        if [ "$DEV_NOTIFY_STATUS" -eq 0 ]; then DEV_NOTIFY_STATUS="$DEV_NOTIFY_TEE_STATUS"; fi
        DEV_NOTIFY_SEND_CLAIMED=0
        if ! dev_notify_is_sent "$DEV_NOTIFY_SENT_MARKER"; then
          dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" exited "$CT_INSTANCE" "$CT_PORT" "$INTERACTIVE_LOG" "$DEV_NOTIFY_STATUS"
        fi
        if [ "$DEV_NOTIFY_SEND_CLAIMED" = 1 ]; then
          dev_notify_stop_stream_waiter "$DEV_NOTIFY_WAITER_PID"
        else
          wait "$DEV_NOTIFY_WAITER_PID" 2>/dev/null || true
        fi
        DEV_NOTIFY_WAITER_PID=
        DEV_NOTIFY_STAGE=exited
        trap - EXIT INT TERM
        dev_notify_exit_report "$DEV_NOTIFY_STATUS"
        exit "$DEV_NOTIFY_STATUS"
      else
        exec "$(dev_app_path)"
      fi
    fi
    ;;
  dev)
    DEV_NOTIFY_STAGE=preflight
    if [ "$CT_DEV_WAIT" -eq 1 ]; then
      HEALTH="$(dev_health_status)"
      HEALTH_FIELDS="$(printf '%s' "$HEALTH" | dev_health_fields 2>/dev/null || true)"
      HEALTH_STATUS="$(printf '%s\n' "$HEALTH_FIELDS" | sed -n '1p')"
      HEALTH_SHA="$(printf '%s\n' "$HEALTH_FIELDS" | sed -n '2p')"
      HEALTH_API_PORT="$(printf '%s\n' "$HEALTH_FIELDS" | sed -n '4p')"
      if [ "$HEALTH_STATUS" = "ok" ] && [ "$HEALTH_API_PORT" = "$CT_PORT" ]; then
        SHA="$HEALTH_SHA"
        if [ -n "$DEV_NOTIFY_TAB" ]; then
          EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
          if SHA="$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH" 2>/dev/null)"; then
            echo "Dev already ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', listener pid $(dev_listener_pid)"
            dev_notify_ready_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CT_INSTANCE" "$CT_PORT" "$SHA" "$(dev_listener_pid)" "$(pwd -P)"
            exit 0
          elif [ -n "$CT_INSTANCE" ]; then
            DEV_NOTIFY_STAGE=stale-health
            DEV_NOTIFY_LOG="/tmp/codeterm-dev${CT_SUFFIX}.log"
            echo "ERROR: named dev listener is stale or wrong-lane; run CODETERM_INSTANCE=$CT_INSTANCE ./build.sh kill before retrying." >&2
            exit 1
          fi
        else
          echo "Dev already ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', listener pid $(dev_listener_pid)"
          exit 0
        fi
      fi
    fi
    DEV_NOTIFY_STAGE=build
    if named_dev_wait_isolated; then
      prepare_named_dev_target
      echo "[isolated-dev] named --wait startup bypasses the canonical dev gateway"
    else
      kill_dev || echo "WARNING: dev cleanup failed; continuing to build gateway acquisition." >&2
      enter_build_gateway "$CMD ${*:2}"
    fi
    prepare_dev_frontend_dist
    ensure_deps
    ct_configure_sccache
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    built_cli_artifact="$(cli_release_binary)"
    built_cli_hash="$(cli_file_sha256_or_status "$built_cli_artifact")"
    # Every dev instance gets its own CLI; production updates its bundled CLI.
    install_cli_to "$CT_DATA_DIR/bin" "$built_cli_artifact" "$built_cli_hash"
    export PATH="$CT_DATA_DIR/bin:$PATH"
    if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
      export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/codeterm"
    else
      export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/codeterm.exe"
    fi
    ensure_real_embed_bundle
    record_dev_target "$(tauri_debug_dir)"
    echo "Starting dev..."
    # Mark the daemon as dev: bundle hashes are bypassed (live edits
    # to <repo>/plugins/<id>/ don't trip the verifier) and the
    # first-launch embedded extract is skipped so ~/.codeterm/plugins/
    # doesn't shadow the workspace.
    export CODETERM_DEV=1
    export CODETERM_WEB_DIST="$(pwd -P)/dist"
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=$CT_CDP_PORT}"
    start_web_watch
    start_dev_static_server

    if [ "$CT_DEV_WAIT" -eq 1 ]; then
      LOG="/tmp/codeterm-dev${CT_SUFFIX}.log"
      DEV_NOTIFY_LOG="$LOG"
      : > "$LOG"
      EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
      PID_FILE="/tmp/codeterm-dev${CT_SUFFIX}.pid.$$"
      if ! start_dev_wait_process "$LOG" > "$PID_FILE"; then
        DEV_NOTIFY_STAGE=launcher-start
        echo "Dev startup failed before the explicit health signal. Last log lines:"
        tail -25 "$LOG"
        exit 1
      fi
      DEV_PID="$(cat "$PID_FILE")"
      rm -f "$PID_FILE"
      echo "Dev pid $DEV_PID, log $LOG. Waiting for /api/health to respond..."
      DEV_NOTIFY_STAGE=launch
      DEADLINE=$(( $(date +%s) + 600 ))
      LAST_STAGE=""
      while : ; do
        STAGE="$(dev_wait_stage "$LOG")"
        if [ "$STAGE" != "$LAST_STAGE" ]; then
          echo "Dev startup stage: $STAGE"
          LAST_STAGE="$STAGE"
        fi
        echo "Dev startup heartbeat: stage=$LAST_STAGE"
        if [ "$(date +%s)" -gt "$DEADLINE" ]; then
          DEV_NOTIFY_STAGE=deadline
          echo "Timeout after 600s during stage: $LAST_STAGE. Last log lines:"
          tail -25 "$LOG"
          if [ -f "/tmp/codeterm-web-watch${CT_SUFFIX}.log" ]; then
            echo "Last web-watch log lines:"
            tail -25 "/tmp/codeterm-web-watch${CT_SUFFIX}.log"
          fi
          exit 1
        fi
        if ! dev_wait_process_alive "$DEV_PID"; then
          DEV_NOTIFY_STAGE=exited
          echo "Dev process exited before reporting ready during stage: $LAST_STAGE. Last log lines:"
          tail -25 "$LOG"
          exit 1
        fi
        HEALTH=$(dev_health_status)
        if [ -n "$HEALTH" ]; then
          STATUS=$(printf '%s' "$HEALTH" | dev_health_fields 2>/dev/null | head -n1 || true)
          if [ "$STATUS" = "ok" ]; then
            if ! SHA=$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH"); then
              DEV_NOTIFY_STAGE=health
              echo "$HEALTH"
              exit 1
            fi
            STABLE=1
            for _ in 1 2 3 4; do
              sleep 2
              HEALTH=$(dev_health_status)
              if ! SHA=$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH" 2>/dev/null); then
                STABLE=0
                break
              fi
            done
            if [ "$STABLE" != "1" ]; then
              DEV_NOTIFY_STAGE=unhealthy
              echo "Dev process became unhealthy immediately after reporting ready. Last log lines:"
              tail -25 "$LOG"
              exit 1
            fi
            if ! dev_frontend_ready; then
              DEV_NOTIFY_STAGE=frontend
              sleep 2
              continue
            fi
            write_dev_provenance "$SHA"
            retain_dev_pdb "$SHA"
            echo "Dev ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', launcher pid $DEV_PID, listener pid $(dev_listener_pid)"
            if [ -n "$DEV_NOTIFY_TAB" ]; then
              dev_notify_ready_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CT_INSTANCE" "$CT_PORT" "$SHA" "$(dev_listener_pid)" "$(pwd -P)"
            fi
            if [ -n "$CT_INSTANCE" ]; then
              echo "Named dev ready; shared build gateway handoff skipped."
            else
              handoff_gateway_to_dev "$DEV_PID"
            fi
            exit 0
          fi
        fi
        sleep 2
      done
    else
      # Retained after the session, not before it: only then is the PDB on disk
      # the one tauri just linked and ran. Nothing relinks until the next dev, so
      # a daemon that died leaves its symbols intact for this copy.
      dev_status=0
      if [ -n "$DEV_NOTIFY_TAB" ]; then
        INTERACTIVE_LOG="/tmp/codeterm-dev${CT_SUFFIX}.interactive.log"
        : > "$INTERACTIVE_LOG"
        DEV_NOTIFY_LOG="$INTERACTIVE_LOG"
        EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
        DEV_NOTIFY_STAGE=launch
        DEV_NOTIFY_WAITER_PID=
        if pnpm tauri dev --no-watch --config "$(dev_conf_path)" 2>&1 | tee -a "$INTERACTIVE_LOG" | report_read_stream "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CMD" launch "$INTERACTIVE_LOG" dev; then
          DEV_NOTIFY_PIPE_STATUS=("${PIPESTATUS[@]}")
        else
          DEV_NOTIFY_PIPE_STATUS=("${PIPESTATUS[@]}")
        fi
        dev_status="${DEV_NOTIFY_PIPE_STATUS[0]}"
        DEV_NOTIFY_TEE_STATUS="${DEV_NOTIFY_PIPE_STATUS[1]}"
        if [ "$dev_status" -eq 0 ]; then dev_status="$DEV_NOTIFY_TEE_STATUS"; fi
        DEV_NOTIFY_SEND_CLAIMED=0
        if ! dev_notify_is_sent "$DEV_NOTIFY_SENT_MARKER"; then
          dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" exited "$CT_INSTANCE" "$CT_PORT" "$INTERACTIVE_LOG" "$dev_status"
        fi
        if [ "$DEV_NOTIFY_SEND_CLAIMED" = 1 ]; then
          dev_notify_stop_stream_waiter "$DEV_NOTIFY_WAITER_PID"
        else
          wait "$DEV_NOTIFY_WAITER_PID" 2>/dev/null || true
        fi
        DEV_NOTIFY_WAITER_PID=
        DEV_NOTIFY_STAGE=exited
      else
        pnpm tauri dev --no-watch --config "$(dev_conf_path)" || dev_status=$?
      fi
      retain_dev_pdb "$(git rev-parse HEAD 2>/dev/null || true)"
      exit "$dev_status"
    fi
    ;;
  e2e)
    export CODETERM_API_URL="http://127.0.0.1:${CT_PORT}"
    cd tests/e2e
    PYTHON_BIN="$(command -v python3 || command -v python)"
    "$PYTHON_BIN" -m pip install -q -r requirements.txt 2>/dev/null
    "$PYTHON_BIN" -m pytest -m "not llm" -v --tb=short "${@:2}"
    ;;
  e2e:llm)
    export CODETERM_API_URL="http://127.0.0.1:${CT_PORT}"
    cd tests/e2e
    PYTHON_BIN="$(command -v python3 || command -v python)"
    "$PYTHON_BIN" -m pip install -q -r requirements.txt 2>/dev/null
    "$PYTHON_BIN" -m pytest -m llm -v --tb=short "${@:2}"
    ;;
  test:all)
    enter_build_gateway "$CMD ${*:2}"
    # Aggregate runner for every enabled NON-LLM test layer. Unit layers always run;
    # dev-dependent API integration tests run only
    # when dev (7686) is healthy, otherwise they SKIP loudly. Exits non-zero iff
    # an EXECUTED layer failed (SKIP never fails the run). Behaviour benches and
    # the suspended Playwright suite are excluded — see .agents/skills/testing/SKILL.md.
    ta_names=(); ta_status=()
    ta_record() { ta_names+=("$1"); ta_status+=("$2"); }
    ta_run() { # name, cmd...
      local name="$1"; shift
      echo; echo "═══════════ layer: $name ═══════════"
      if "$@"; then ta_record "$name" PASS; else ta_record "$name" FAIL; fi
    }

    ta_run "rust-unit"  scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib
    ta_run "api-effects" node scripts/check-api-effect-dependencies.mjs
    ta_run "ts-unit"    pnpm -s test:unit
    ta_run "ts-vitest"  node node_modules/vitest/vitest.mjs run
    ta_run "plugin"     pnpm -s test:plugins

    PYTHON_BIN="$(command -v python3 || command -v python)"
    DEV_STATUS=$(dev_health_status | "$PYTHON_BIN" -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    if [ "$DEV_STATUS" = "ok" ]; then
      ta_run "e2e-pytest" bash -c "cd tests/e2e && '$PYTHON_BIN' -m pip install -q -r requirements.txt 2>/dev/null && CODETERM_API_URL=http://127.0.0.1:${CT_PORT} '$PYTHON_BIN' -m pytest -m 'not llm' -q"
    else
      echo
      echo ">>> SKIPPED e2e-pytest — dev 7686 not healthy (start with ./build.sh dev)"
      ta_record "e2e-pytest" SKIP
    fi

    echo; echo "════════════ test:all summary ════════════"
    ta_rc=0
    for i in "${!ta_names[@]}"; do
      printf '  %-18s %s\n' "${ta_names[$i]}" "${ta_status[$i]}"
      [ "${ta_status[$i]}" = "FAIL" ] && ta_rc=1
    done
    echo "──────────────────────────────────────────"
    if [ "$ta_rc" -eq 0 ]; then
      echo "✅ all executed layers passed"
    else
      echo "❌ one or more executed layers failed"
    fi
    exit "$ta_rc"
    ;;
  *)
    echo "Usage: ./build.sh help"
    exit 1
    ;;
esac
