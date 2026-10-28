#!/usr/bin/env bash
# Domios build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh dev-prebuilt --wait — restart a provenance-validated dev build without Cargo
#   ./build.sh dev-server — rebuild/reload the selected DEV server, retaining its window
#   ./build.sh check     — integration-owner cargo check
#   ./build.sh frontend  — production frontend typecheck + bundle
#   ./build.sh release-inputs — build target-aware frontend/CLI bundle inputs
#   ./build.sh release-bundle [--fresh-target] [tauri args...] — prepare and bundle in one gateway session
#   ./build.sh release-preflight — run local PR checks and verify release inputs
#   ./build.sh worktree-check — explicit isolated, nonincremental checkout check
#   ./build.sh build     — production build
#   ./build.sh install-local — incremental production build + self-install, then resume the calling tab
#   ./build.sh bootstrap — create/build bundled CLI for fresh clones
#   ./build.sh qa-prep   — one-shot: dist + sidecars for a fresh isolated worktree (no tsc gate)
#   ./build.sh cli       — build CLI and install to the active dev data directory
#   ./build.sh fmt [--base REF] — format changed Rust files and apply changed-file Biome safe fixes
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh e2e       — run non-LLM E2E tests against dev (port 7686)
#   ./build.sh e2e:llm   — run LLM E2E tests against dev (real provider tokens)
#   source build.sh env  — load MSVC env vars into current shell (Windows only)

codeterm_build_main() {
set -e

source scripts/dev-notify.sh
source scripts/firebase-build-config.sh
source scripts/local-build-env.sh
dev_notify_capture_parent

CMD="${1:-dev}"
dev_build_phase() {
  [ "${CODETERM_BUILD_TIMINGS:-0}" = 1 ] || return 0
  case "${CMD:-}" in dev|dev-back|dev-prebuilt|dev-server) ;; *) return 0 ;; esac
  local timestamp="${EPOCHREALTIME:-}"
  [ -n "$timestamp" ] || timestamp="$(node -p 'Date.now()/1000')"
  printf '[dev-phase] time=%s pid=%s phase=%s\n' "$timestamp" "${BASHPID:-$$}" "$1" >&2
}
dev_build_phase startup.begin
if [ "$CMD" = dev-back ]; then
  export DEV_FRONT_EXTERNAL=1
  set -- dev --wait "${@:2}"
  CMD=dev
fi
source scripts/lane-rust-guard.sh
require_canonical_rust_tests "$CMD" "${@:2}" || exit $?
if ct_uses_local_build_env; then ct_load_local_build_env || exit 2; fi
if [ "$CMD" = test-related ]; then
  local related_shell="$BASH"
  if command -v cygpath >/dev/null 2>&1; then related_shell="$(cygpath -w "$BASH")"; fi
  CT_TEST_RELATED_SHELL="$related_shell" node scripts/test-related.mjs "${@:2}"
  return $?
fi
CT_DEV_WAIT=0
DEV_NOTIFY_TAB="${DEV_NOTIFY_TAB:-${BUILD_NOTIFY_TAB:-}}"
DEV_NOTIFY_TARGET_KIND="${DEV_NOTIFY_TARGET_KIND:-none}"
DEV_NOTIFY_TARGET_MESH="${DEV_NOTIFY_TARGET_MESH:-}"

# Long operations stream one receipt through the shell's live Domios return route.
case "$CMD" in
  dev-server|process-probe|qa-lock-storm|feature-cycle|lane-check|embedding-parity|check|check-queue|check-tests|test-one|nextest|nextest-archive|release-preflight|release-rust|install-local|build|test|verify|fmt-check)
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
      if [ "$BUILD_NOTIFY_GIVEN" = 1 ] && [ -z "$DEV_NOTIFY_PARENT_CLI" ]; then
        echo "ERROR: codeterm was not available on the parent PATH for --notify-tab" >&2
        exit 127
      fi
      if [ "$BUILD_NOTIFY_GIVEN" = 1 ] || [ -n "${DEV_NOTIFY_PARENT_CODETERM_TAB_ID:-}" ] || [ -n "${DEV_NOTIFY_PARENT_CODETERM_PARENT_TAB_ID:-}" ]; then
        dev_notify_resolve_target "${BUILD_NOTIFY_TAB:-}"
      fi
      if [ "${DEV_NOTIFY_TARGET_KIND:-none}" != none ]; then
        BUILD_NOTIFY_LOG="$(mktemp "${TMPDIR:-/tmp}/codeterm-build-$CMD.XXXXXX")"
        BUILD_NOTIFY_MARKER="$BUILD_NOTIFY_LOG.sent"
        rm -rf "$BUILD_NOTIFY_MARKER"
        BUILD_NOTIFY_STATUS=0
        report_run_stream "$BUILD_NOTIFY_MARKER" "$DEV_NOTIFY_TAB" "$CMD" "$CMD ${BUILD_NOTIFY_ARGS[*]:-}" "$BUILD_NOTIFY_LOG" \
          env BUILD_NOTIFY_CHILD=1 DEV_NOTIFY_TAB="$DEV_NOTIFY_TAB" DEV_NOTIFY_TARGET_KIND="$DEV_NOTIFY_TARGET_KIND" \
            DEV_NOTIFY_TARGET_MESH="$DEV_NOTIFY_TARGET_MESH" bash "$0" "$CMD" ${BUILD_NOTIFY_ARGS[@]+"${BUILD_NOTIFY_ARGS[@]}"} || BUILD_NOTIFY_STATUS=$?
        if [ "$BUILD_NOTIFY_STATUS" -eq 0 ]; then
          BUILD_NOTIFY_BODY="$(dev_notify_new_body_file)"
          printf 'OK label=%s step=%s\nLog: %s\n' "$CMD" "$CMD ${BUILD_NOTIFY_ARGS[*]:-}" "$BUILD_NOTIFY_LOG" > "$BUILD_NOTIFY_BODY"
          dev_notify_send_once "$BUILD_NOTIFY_MARKER" "$DEV_NOTIFY_TAB" "$BUILD_NOTIFY_BODY" || true
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
  if [ "$DEV_NOTIFY_SEEN" = "1" ] && [ -z "$DEV_NOTIFY_PARENT_CLI" ]; then
    echo "ERROR: codeterm was not available on the parent PATH for --notify-tab" >&2
    exit 127
  fi
  if [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ] && { [ "$DEV_NOTIFY_SEEN" = "1" ] || [ -n "$DEV_NOTIFY_TAB" ] || [ -n "${DEV_NOTIFY_PARENT_CODETERM_TAB_ID:-}" ]; }; then
    dev_notify_resolve_target "$DEV_NOTIFY_TAB"
  fi
  if [ "${DEV_NOTIFY_TARGET_KIND:-none}" != none ]; then
    DEV_NOTIFY_SENT_MARKER="${DEV_NOTIFY_SENT_MARKER:-${TMPDIR:-/tmp}/codeterm-dev-notify.$$.sent}"
    if [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ]; then rm -rf "$DEV_NOTIFY_SENT_MARKER"; fi
    DEV_NOTIFY_STAGE=setup
    trap 'dev_notify_exit_report "$?"' EXIT
  fi
fi

if [ -n "$DEV_NOTIFY_TAB" ] && [ "${DEV_NOTIFY_STREAM_CHILD:-0}" != 1 ] && [ "${BUILD_NOTIFY_CHILD:-0}" != 1 ]; then
  DEV_NOTIFY_STREAM_LOG="$(mktemp "${TMPDIR:-/tmp}/codeterm-dev-stream.XXXXXX")"
  DEV_NOTIFY_EARLY_MARKER="$DEV_NOTIFY_SENT_MARKER"
  trap - EXIT
  DEV_NOTIFY_STREAM_STATUS=0
  report_run_stream "$DEV_NOTIFY_EARLY_MARKER" "$DEV_NOTIFY_TAB" "$CMD" startup "$DEV_NOTIFY_STREAM_LOG" \
    env DEV_NOTIFY_STREAM_CHILD=1 DEV_NOTIFY_SENT_MARKER="$DEV_NOTIFY_SENT_MARKER" DEV_NOTIFY_TAB="$DEV_NOTIFY_TAB" \
      DEV_NOTIFY_TARGET_KIND="$DEV_NOTIFY_TARGET_KIND" DEV_NOTIFY_TARGET_MESH="$DEV_NOTIFY_TARGET_MESH" bash "$0" "$@" || DEV_NOTIFY_STREAM_STATUS=$?
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
    # Visual Studio spells some library directories ARM64.
    if [ "${leaf,,}" != "${want,,}" ]; then
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
    discovered="$("$vswhere" -latest -products '*' -version '[17.0,)' \
      -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
      -property installationPath | tr -d '\r' | tr '\\' '/')" || discovered=""
  fi
  if [ -n "$discovered" ] && [ -d "$discovered/VC/Tools/MSVC" ]; then
    printf '%s\n' "$discovered"
  else
    printf '%s\n' "$preferred"
  fi
}

ct_msvc_native_arm64_env() {
  [ "${VSCMD_ARG_HOST_ARCH:-}" = arm64 ] && [ "${VSCMD_ARG_TGT_ARCH:-}" = arm64 ] && [ -n "${LIB:-}" ] || {
    echo "ERROR: hosted arm64 requires a native arm64 Visual Studio environment" >&2; return 2;
  }
  local native_tools native_compiler
  native_tools="$(cygpath -m "${VCToolsInstallDir:?Native MSVC tools directory required}")"
  native_tools="${native_tools%/}"
  native_compiler="$native_tools/bin/Hostarm64/arm64"
  [ -f "$native_compiler/cl.exe" ] && [ -f "$native_compiler/lib.exe" ] && [ -f "$native_compiler/link.exe" ] || {
    echo "ERROR: native arm64 MSVC tools are missing" >&2; return 2;
  }
  ct_assert_msvc_lib_arch arm64 || return
  # rustc filters link.exe progress output only for a bare "link.exe", so MSVC must win the PATH lookup over Git's link.
  export PATH="$(cygpath -u "$native_compiler"):$PATH"
  export CC="$native_compiler/cl.exe" CXX="$native_compiler/cl.exe"
  export CC_aarch64_pc_windows_msvc="$CC" CXX_aarch64_pc_windows_msvc="$CXX"
  export AR_aarch64_pc_windows_msvc="$native_compiler/lib.exe"
  export npm_config_script_shell="$(cygpath -w "$(command -v bash)")"
}

setup_env() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    if [ "$CMD" = release-arm64-compile ]; then
      # An x64 host cross-compiles: rustc and cc locate MSVC's tools per target. An inherited developer
      # environment (the runner's x64 one) would hand x64 libraries to the ARM64 link, so it is dropped.
      if [ "${VSCMD_ARG_HOST_ARCH:-}" != arm64 ]; then
        unset VSCMD_ARG_HOST_ARCH VSCMD_ARG_TGT_ARCH VSCMD_VER VCINSTALLDIR VCToolsInstallDir LIB LIBPATH INCLUDE
        export npm_config_script_shell="$(cygpath -w "$(command -v bash)")"
        return
      fi
      ct_msvc_native_arm64_env
      return
    fi
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

initialize_build_identity() {
  local full_hash short_hash
  CT_DEV_PROVENANCE_SHA=
  if [ "$CMD" = dev-prebuilt ] && [ -n "${CT_DEV_HANDOFF_SHA:-}" ]; then
    full_hash="$CT_DEV_HANDOFF_SHA"
    short_hash="${CT_DEV_HANDOFF_HASH:-}"
    CT_DEV_PROVENANCE_SHA="$CT_DEV_HANDOFF_SHA"
  else
    full_hash="$(git rev-parse HEAD 2>/dev/null || true)"
    short_hash="$(git rev-parse --short "$full_hash" 2>/dev/null || true)"
  fi
  unset CT_DEV_HANDOFF_SHA CT_DEV_HANDOFF_HASH
  unset CODETERM_BUILD_SHA CODETERM_BUILD_HASH CODETERM_FRONTEND_BUILD_HASH
  if [[ ! "$full_hash" =~ ^[0-9a-f]{40}$ ]] ||
      [[ ! "$short_hash" =~ ^[0-9a-f]{7,40}$ ]] || [[ "$full_hash" != "$short_hash"* ]]; then
    CT_DEV_PROVENANCE_SHA=
    case "$CMD" in
      dev|dev-prebuilt|dev-server|build|install-local)
        echo "ERROR: $CMD requires a valid build identity from Git or its internal launch handoff" >&2
        return 1
        ;;
    esac
    return 0
  fi
  export CODETERM_BUILD_SHA="$full_hash"
  export CODETERM_BUILD_HASH="$short_hash"
  export CODETERM_FRONTEND_BUILD_HASH="$short_hash"
}
initialize_build_identity

# --- Kill THIS dev instance only (never prod, never another worktree) ---
# Scoped strictly to $CT_PORT / $CT_VITE_PORT / this instance's screen+pidfile.
# No global pgrep on target/debug/domios-app — that would take down every worktree.
kill_dev() {
  if [ "$CT_PORT" = "7685" ]; then
    echo "ERROR: refusing to kill prod Domios port 7685" >&2
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
    if [ "${DEV_FRONT_EXTERNAL:-0}" != 1 ]; then
      vite_pids="$(lsof -ti:"$CT_VITE_PORT" 2>/dev/null || true)"
      [ -z "$vite_pids" ] || echo "$vite_pids" | xargs kill -9 2>/dev/null || true
    fi
    if [ -f "$CT_DATA_DIR/daemon.pid" ]; then
      kill "$(cat "$CT_DATA_DIR/daemon.pid")" 2>/dev/null || true
      rm -f "$CT_DATA_DIR/daemon.pid"
    fi
  else
    node scripts/dev-rebuild-stop.mjs "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$CT_INSTANCE" "$(dev_app_path)" || return
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    local vite_pid
    # dev-back keeps the separately owned dev-front (Vite HMR) process alive.
    if [ "${DEV_FRONT_EXTERNAL:-0}" != 1 ]; then
      vite_pid=$(netstat -ano 2>/dev/null | grep ":$CT_VITE_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
      if [ -n "$vite_pid" ] && [ "$vite_pid" != "0" ]; then
        taskkill //PID "$vite_pid" //T //F 2>/dev/null || true
      fi
    fi
    windows_dev_leftover_cleanup
  fi
}

stop_dev_before_replacement() {
  if [[ "$OSTYPE" == msys* ]] || [[ "$OSTYPE" == cygwin* ]]; then
    node scripts/dev-rebuild-stop.mjs "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$CT_INSTANCE" "$(dev_app_path)" || return
    kill_web_watch
    kill_dev_static_server
  else
    kill_dev
  fi
}

request_dev_clean_restart() {
  local token cli="$CT_DATA_DIR/bin/codeterm"
  [ -x "$cli" ] || cli="$cli.exe"
  [ -x "$cli" ] || return 1
  token=$(env -u CODETERM_API_TOKEN -u CODETERM_API_URL -u CODETERM_TAB_ID -u CODETERM_SESSION_ID -u CODETERM_SESSION_BINDING_NONCE \
    CODETERM_DATA_DIR="$CT_DATA_DIR" CODETERM_API_PORT="$CT_PORT" \
    "$cli" system api-token) || return 1
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
  for package in codeterm-service-core codeterm-account codeterm-billing codeterm-frontend-bundle codeterm-types codeterm-wire codeterm-sentinel codeterm-decision codeterm-provider-signals codeterm-util codeterm-mesh codeterm-mem codeterm-plan codeterm-bridge codeterm-pty codeterm-plugin codeterm-plugins; do
    TEST_CAP_SECS="$test_cap_secs" scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p "$package" --lib
  done
}

rust_test_package() {
  case "$1" in
    app|service-core) printf '%s\n' codeterm-service-core ;;
    root|shell|codeterm) printf '%s\n' codeterm ;;
    domios-server|codeterm-*) printf '%s\n' "$1" ;;
    api|socket|commands|sentinel-host|billing|account|analytics|plan|pty|util|browser|types|wire|sentinel|decision|provider-signals|storage|mem|mem-store|paths|mesh|plugin|plugins|agent-detect|notification-core|repowatch|usage|blueprint-types|local-recording|cli)
      printf 'codeterm-%s\n' "$1" ;;
    *) echo "Unknown test scope: $1" >&2; return 2 ;;
  esac
}

run_nextest_scope() {
  local CARGO_TARGET_DIR="${CARGO_TARGET_DIR-}"
  [ "$#" -le 2 ] || {
    echo "usage: ./build.sh nextest <scope> [filterset]" >&2
    return 2
  }
  local scope="${1:?nextest requires a scope}"
  local filterset="${2:-}"
  local manifest="src-tauri/Cargo.toml"
  local package=""
  local targets=(--no-default-features)
  local filter_args=() run_ignored_args=()

  case "$scope:$filterset" in
    workspace:*|app:*|codeterm:*|service-core:*|codeterm-service-core:*|shell:*|*rdeps\(*)
      prepare_dev_frontend_dist "$(pwd -P)"
      prepare_bundle_stubs
      ;;
  esac

  case "$scope" in
    workspace)
      # One feature resolution for every crate, so shared dependencies compile once.
      local lane_args cli_args=() arg
      lane_args="$(node scripts/rust-test-lanes.mjs workspace)" || return $?
      targets=()
      while IFS= read -r arg; do targets+=("$arg"); done <<< "$lane_args"
      local workspace_target cli_target
      workspace_target="$(node scripts/rust-test-lanes.mjs workspace target)" || return $?
      cli_target="$(node scripts/rust-test-lanes.mjs cli target)" || return $?
      lane_args="$(node scripts/rust-test-lanes.mjs cli)" || return $?
      while IFS= read -r arg; do cli_args+=("$arg"); done <<< "$lane_args"
      CARGO_TARGET_DIR="$cli_target" cargo test --manifest-path cli/Cargo.toml "${cli_args[@]}" \
        --locked --no-run --message-format=json >/dev/null || return $?
      export CARGO_TARGET_DIR="$workspace_target"
      ;;
    app|codeterm|service-core|codeterm-service-core)
      configure_app_test_cargo nextest
      manifest="$CT_APP_TEST_MANIFEST"
      package="$CT_APP_TEST_PACKAGE"
      targets=("${CT_APP_TEST_TARGET_ARGS[@]}")
      ;;
    domios-server) package="domios-server"; targets+=(--lib) ;;
    shell) package="codeterm"; targets+=(--lib) ;;
    devtools|codeterm-devtools) package="codeterm-devtools"; targets+=(--test main) ;;
    mem-store|codeterm-mem-store) package="codeterm-mem-store"; targets=(--lib) ;;
    paths|codeterm-paths) package="codeterm-paths"; targets=(--lib) ;;
    process-identity|codeterm-process-identity) package="codeterm-process-identity"; targets=(--lib) ;;
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
    mem-postgres|codeterm-mem-postgres)
      package="codeterm-mem-postgres"; targets+=(--lib)
      filter_args+=(--status-level all --final-status-level all)
      if [ -n "${DOMIOS_TEST_POSTGRES_URL:-}" ]; then
        run_ignored_args=(--run-ignored all)
      else
        echo "Postgres contracts are ignored: DOMIOS_TEST_POSTGRES_URL is unset."
      fi
      ;;
    mesh|codeterm-mesh) package="codeterm-mesh"; targets+=(--lib) ;;
    plugin|codeterm-plugin) package="codeterm-plugin"; targets+=(--lib) ;;
    plugins|codeterm-plugins) package="codeterm-plugins"; targets+=(--lib) ;;
    cli|codeterm-cli)
      manifest="cli/Cargo.toml"
      local cli_selection cli_arg
      cli_selection="$(node scripts/rust-test-lanes.mjs cli)" || return $?
      targets=()
      while IFS= read -r cli_arg; do targets+=("$cli_arg"); done <<< "$cli_selection"
      cli_selection="$(node scripts/rust-test-lanes.mjs cli target)" || return $?
      export CARGO_TARGET_DIR="$cli_selection"
      ;;
    pty-fork) manifest="src-tauri/pty-fork/Cargo.toml" ;;
    codeterm-*) package="$scope"; targets+=(--lib) ;;
    *)
      echo "Unknown nextest scope: $scope" >&2
      return 2
      ;;
  esac

  if [ "$scope" = "app" ] || [ "$scope" = "codeterm" ] || [ "$scope" = "service-core" ] || [ "$scope" = "codeterm-service-core" ] || [ "$scope" = "shell" ]; then
    prepare_exceptional_worktree_target
  fi

  if [ -n "$filterset" ]; then
    filter_args+=(--filter-expr "$filterset")
  fi
  if [ -n "$package" ]; then
    if [[ "$filterset" == *"rdeps("* && "$package" != codeterm-cli ]]; then
      # The CLI shares the workspace but keeps its own test lane.
      targets+=(--workspace --exclude codeterm-cli)
    else
      targets+=(-p "$package")
    fi
  fi

  local timing_dir run_exit=0 guard_exit=0 alone_exit=0 reuse_args=() confirmation_args=() archive=""
  if [ -n "${CT_BUILD_ARTIFACT_DIR:-}" ]; then timing_dir="$(mktemp -d "$CT_BUILD_ARTIFACT_DIR/nextest.XXXXXX")"; else timing_dir="$(mktemp -d)"; fi
  if [ "$package" = codeterm-service-core ] && [[ "$filterset" != *'rdeps('* ]] && find_app_test_archive; then
    archive="$CT_APP_TEST_ARCHIVE"
    configure_app_archive_run "$filterset" || return $?
    reuse_args=("${CT_APP_ARCHIVE_RUN_ARGS[@]}")
    filter_args=()
    confirmation_args=(--archive-file "$archive")
  else
  if declare -F prepare_nextest_metadata >/dev/null; then
    prepare_nextest_metadata "$timing_dir" "$manifest" "${targets[@]}" || return $?
  else
  if cargo nextest list --manifest-path "$manifest" "${targets[@]}" \
    --locked --list-type binaries-only --message-format json > "$timing_dir/binaries.json"; then
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
  fi
  reuse_args=(--binaries-metadata "$timing_dir/binaries.json" --cargo-metadata "$timing_dir/cargo.json")
  confirmation_args=("$timing_dir/binaries.json" "$timing_dir/cargo.json")
  fi
  if CARGO_TEST_CMD="cargo nextest run" CARGO_TEST_BUILD_CMD="" NEXTEST_STATUS_LEVEL=pass TEST_RAW_OUTPUT=1 \
    scripts/test-capped.sh \
    "${reuse_args[@]}" \
    "${filter_args[@]}" "${run_ignored_args[@]}" --no-fail-fast --no-tests="${NEXTEST_NO_TESTS_MODE:-fail}" > "$timing_dir/parallel.log" 2>&1; then
    run_exit=0
  else
    run_exit=$?
  fi
  node "${CT_GATEWAY_SCRIPT_ROOT:-.}/scripts/build-gateway-output.mjs" test-summary "$timing_dir/parallel.log" "$run_exit"
  if [ "${SLOW_UNIT_TESTS_MEASUREMENT:-}" = "off" ]; then
    echo "slow-unit measurement skipped (SLOW_UNIT_TESTS_MEASUREMENT=off)"
  elif [ "${SLOW_UNIT_TESTS_MEASUREMENT:-}" = "alone" ]; then
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
        "${confirmation_args[@]}" "$timing_dir/alone.log"; then
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
  [ -n "${CT_BUILD_ARTIFACT_DIR:-}" ] || rm -rf "$timing_dir"
  [ "$run_exit" -eq 0 ] || return "$run_exit"
  [ "$alone_exit" -eq 0 ] || return "$alone_exit"
  return "$guard_exit"
}

# Every release-tier Rust runner from tests/catalogue.json, one nextest scope each, no fail-fast across runners.
run_release_rust_tests() {
  local plan scope ids budget no_tests filterset status total=0
  local red=()
  plan="$(node scripts/ci-rust-archives.mjs release-plan "$@")" || return $?
  [ -n "$plan" ] || { echo "release-rust: the catalogue selects no release Rust runner" >&2; return 1; }
  prepare_exceptional_worktree_target
  while IFS=$'\t' read -r -u 3 scope ids budget no_tests filterset; do
    total=$((total + 1))
    echo "== release-rust: $ids (./build.sh nextest $scope${filterset:+ '$filterset'}, cap ${TEST_CAP_SECS:-$budget}s, no-tests=$no_tests)"
    if TEST_CAP_SECS="${TEST_CAP_SECS:-$budget}" NEXTEST_NO_TESTS_MODE="$no_tests" run_nextest_scope "$scope" ${filterset:+"$filterset"}; then
      echo "release-rust: GREEN $ids"
    else
      status=$?
      echo "release-rust: RED $ids (exit $status)"
      red+=("$ids")
    fi
  done 3<<< "$plan"
  if [ "${#red[@]}" -gt 0 ]; then
    echo "release-rust: ${#red[@]} of $total runner group(s) red: ${red[*]}" >&2
    return 1
  fi
  echo "release-rust: all $total runner group(s) green"
}

case "$CMD" in
  gateway-*|help|gc|cache-status|cache-verify|cache-release|cache-stats|build-history|frontend-build) ;;
  *) setup_env ;;
esac

# --- Per-worktree dev instance isolation ---
# CODETERM_DEV_INSTANCE names a dev instance so multiple worktrees can
# run ./build.sh dev concurrently without colliding. Unset = canonical defaults
# (.domios-dev / 7686 / 1420). See AGENTS.md "Process safety".
CT_INSTANCE="${CODETERM_INSTANCE:-}"
CT_PORT="${CODETERM_API_PORT:-7686}"
CT_VITE_PORT="${CODETERM_VITE_PORT:-}"
CT_CDP_PORT="${CODETERM_CDP_PORT:-}"
CT_INSTANCE_HASH=""
case "$CMD" in
  dev|dev-prebuilt|dev-server|dev-front|kill|e2e|e2e:llm)
    CT_DEV_SELECTION="$(node scripts/dev-instance-policy.mjs)" || exit $?
    CT_INSTANCE="${CT_DEV_SELECTION%:*}"
    CT_PORT="${CT_DEV_SELECTION##*:}"
    CT_VITE_PORT="${CODETERM_DEV_VITE_PORT:-}"
    CT_CDP_PORT="${CODETERM_DEV_CDP_PORT:-}"
    export CODETERM_DEV_INSTANCE="$CT_INSTANCE"
    export CODETERM_DEV_API_PORT="$CT_PORT"
    unset CODETERM_API_URL CODETERM_API_TOKEN CODETERM_SOCKET_PATH CODETERM_DATA_DIR
    ;;
esac
if [ -n "$CT_INSTANCE" ]; then
  CT_SLUG=$(printf '%s' "$CT_INSTANCE" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g')
  if [ "$CT_INSTANCE" = "$CT_SLUG" ]; then
    CT_INSTANCE_KEY="$CT_SLUG"
  else
    CT_INSTANCE_HASH="$(node -e 'process.stdout.write(require("node:crypto").createHash("sha256").update(process.argv[1]).digest("hex").slice(0,10))' "$CT_INSTANCE")"
    CT_INSTANCE_KEY="$CT_SLUG-x$CT_INSTANCE_HASH"
  fi
  CT_SUFFIX="-$CT_INSTANCE_KEY"
  CT_IDENTIFIER="com.domios.dev.$(printf '%s' "$CT_SLUG" | tr '-' '.')"
  if [ -n "$CT_INSTANCE_HASH" ]; then
    CT_IDENTIFIER="$CT_IDENTIFIER.x$CT_INSTANCE_HASH"
  fi
  # Stable per-instance vite/CDP ports use the isolated instance key.
  CT_OFFSET=$(printf '%s' "$CT_INSTANCE_KEY" | od -An -tu1 | awk '{for(i=1;i<=NF;i++)s+=$i} END{print (s%200)+1}')
  CT_VITE_PORT="${CT_VITE_PORT:-$((1420 + CT_OFFSET))}"
  CT_CDP_PORT="${CT_CDP_PORT:-$((9223 + CT_OFFSET))}"
else
  CT_SLUG=""
  CT_SUFFIX=""
  CT_IDENTIFIER="com.domios.dev"
  CT_VITE_PORT="${CT_VITE_PORT:-1420}"
  CT_CDP_PORT="${CT_CDP_PORT:-9223}"
fi
CT_DATA_DIR="$HOME/.domios-dev${CT_SUFFIX}"
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
    if [ "$arg" = "--wait-build" ]; then
      export CODETERM_BUILD_GATEWAY_MODE=queue
    else
      BUILD_ARGS+=("$arg")
    fi
  done
  set -- "$CMD" "${BUILD_ARGS[@]}"
  export CODETERM_BUILD_GATEWAY_MODE="${CODETERM_BUILD_GATEWAY_MODE:-fail-fast}"
fi

named_dev_target_dir() {
  printf '%s\n' "$(pwd -P)/src-tauri/target/dev-instances/$CT_INSTANCE_KEY"
}

named_dev_isolated() {
  [ -n "$CT_INSTANCE" ]
}

prepare_named_dev_target() {
  export CARGO_TARGET_DIR="$(named_dev_target_dir)"
  configure_named_dev_cache
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  echo "[isolated-dev] cargo target=$CARGO_TARGET_DIR"
}

configure_named_dev_cache() {
  export CARGO_INCREMENTAL=0
  export CODETERM_PORTABLE_COMPILER_CACHE=1
}

if [ "${CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY:-0}" = "1" ]; then
  if [ -n "$CT_INSTANCE" ]; then
    printf 'instance=%s\ndata_dir=%s\napi_port=%s\nvite_port=%s\ncdp_port=%s\nshared_gateway_handoff=skipped\n' \
      "$CT_INSTANCE" "$CT_DATA_DIR" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT"
  else
    printf 'instance=\ndata_dir=%s\napi_port=%s\nvite_port=%s\ncdp_port=%s\nshared_gateway_handoff=required\n' \
      "$CT_DATA_DIR" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT"
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
  if named_dev_isolated; then
    printf 'mode=isolated-named\ntarget=%s\nbuild_gateway=required\n' "$(named_dev_target_dir)"
  else
    printf 'mode=shared\nbuild_gateway=required\n'
  fi
  exit 0
fi

source scripts/cache-policy.sh
source scripts/build-gateway.sh
source scripts/nextest-archive.sh
source scripts/lane-check.sh
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

wait_build_gateway_reader() {
  local pid="$1" artifact="$2" deadline="$3" token watchdog status=0
  token="$(ct_gateway_process_start_token "$pid" 2>/dev/null || true)"
  node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-reader-deadline.mjs" "$pid" "$token" "$deadline" "$artifact" &
  watchdog=$!
  wait "$pid" || status=$?
  kill "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true
  return "$status"
}

finish_build_gateway_logging() {
  local reader_status=0 trace_status="${CT_BUILD_GATEWAY_TRACE_DEGRADED:-0}" deadline=10000
  if [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = fixture ]; then
    deadline="${CT_BUILD_GATEWAY_READER_TIMEOUT_MS:-10000}"
  fi
  exec 2>&4
  if [ -n "${CT_BUILD_GATEWAY_TRACE_READER_PID:-}" ]; then
    wait_build_gateway_reader "$CT_BUILD_GATEWAY_TRACE_READER_PID" "$CT_BUILD_GATEWAY_TRACE_PATH" "$deadline" || trace_status=$?
    node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-output.mjs" fingerprint-summary "$CT_BUILD_GATEWAY_TRACE_PATH"
  fi
  exec 1>&3
  if [ -n "${CT_BUILD_GATEWAY_LOG_READER_STATUS:-}" ]; then
    reader_status="$CT_BUILD_GATEWAY_LOG_READER_STATUS"
  else
    wait_build_gateway_reader "$CT_BUILD_GATEWAY_LOG_READER_PID" "$CT_BUILD_GATEWAY_LOG_PATH" "$deadline" || reader_status=$?
  fi
  rm -f -- "${CT_BUILD_GATEWAY_LOG_FIFO:-}"
  [ -z "${CT_BUILD_GATEWAY_TRACE_FIFO:-}" ] || rm -f -- "$CT_BUILD_GATEWAY_TRACE_FIFO"
  exec 3>&- 4>&-
  [ "$trace_status" = 0 ] || return "$trace_status"
  return "$reader_status"
}

complete_build_gateway_before_handoff() {
  local reader_status=0 finish_status=0 outcome=""
  trap - EXIT INT TERM
  if [ -n "${CT_BUILD_GATEWAY_LOG_PATH:-}" ]; then
    printf 'Build gateway job %s build exit: 0\n' "$CT_BUILD_JOB_ID"
    finish_build_gateway_logging || reader_status=$?
    if [ "$reader_status" -ne 0 ]; then
      printf 'WARNING: build gateway logging path broke before launch handoff (reader exit %s); build exit remains 0; log: %s\n' \
        "$reader_status" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
      outcome=logging_degraded
    fi
  fi
  ct_gateway_finish 0 0 "$outcome" || finish_status=$?
  return "$finish_status"
}

build_gateway_logging_on_exit() {
  local captured_status="$?" command_status="" command_status_text=UNKNOWN
  local summary_status reader_status finish_status
  trap - EXIT INT TERM
  set +e
  release_stable_build_path
  if [ "${CT_BUILD_GATEWAY_COMMAND_STATUS_KNOWN:-0}" = "1" ] && [ "${CT_GATEWAY_CANCELLED:-0}" != "1" ]; then
    command_status="$captured_status"
    command_status_text="$captured_status"
  fi
  printf 'Build gateway job %s exit: %s\n' "$CT_BUILD_JOB_ID" "$command_status_text"
  summary_status=$?
  finish_build_gateway_logging
  reader_status=$?
  if [ "$summary_status" -ne 0 ] || [ "$reader_status" -ne 0 ]; then
    if [ -n "$command_status" ]; then
      printf 'WARNING: build gateway logging path broke (reader exit %s); command exit was %s and is preserved; log: %s\n' \
        "$reader_status" "$command_status" "$CT_BUILD_GATEWAY_LOG_PATH" >&2
      ct_gateway_finish "$command_status" "$command_status" logging_degraded
      finish_status=$?
      [ "$finish_status" -eq 0 ] || exit "$finish_status"
      exit "$command_status"
    fi
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

# Rust cache keys hash the absolute cwd, so each worktree misses another's entries.
# Windows-only: Unix getcwd resolves symlinks, so no fixed alias can give rustc a stable cwd.
enter_stable_build_path() {
  CT_REAL_WORKTREE="$(cd "${1:-.}" && pwd -P)"
  export CT_REAL_WORKTREE
  [ "${CODETERM_STABLE_BUILD_PATH:-1}" = 1 ] || return 0
  [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = production ] || return 0
  case "$OSTYPE" in msys*|cygwin*|win32) ;; *) return 0 ;; esac
  local drive="${CODETERM_STABLE_BUILD_DRIVE:-X}" lower
  if ! ct_gateway_owned_by_caller; then
    echo "ERROR: stable build mapping requires the exact production gateway owner" >&2
    return 75
  fi
  lower="$(printf '%s' "$drive" | tr 'A-Z' 'a-z')"
  MSYS2_ARG_CONV_EXCL='*' subst "$drive:" /D >/dev/null 2>&1 || true
  if ! MSYS2_ARG_CONV_EXCL='*' subst "$drive:" "$(cygpath -w "$CT_REAL_WORKTREE")" >/dev/null 2>&1; then
    echo "[cache-policy] stable build path unavailable; building from $CT_REAL_WORKTREE" >&2
    return 0
  fi
  CT_STABLE_BUILD_DRIVE_MAPPED="$drive"
  CT_STABLE_BUILD_JOB_ID="$CT_BUILD_JOB_ID"
  cd "/$lower"
  echo "[cache-policy] stable build path $drive:\ -> $CT_REAL_WORKTREE"
}

release_stable_build_path() {
  [ -n "${CT_STABLE_BUILD_DRIVE_MAPPED:-}" ] || return 0
  [ "${CT_BUILD_GATEWAY_SCOPE:-production}" = production ] || return 0
  [ -n "${CT_STABLE_BUILD_JOB_ID:-}" ] || return 0
  [ "$(cat "$(ct_gateway_dir)/lock/job_id" 2>/dev/null)" = "${CT_STABLE_BUILD_JOB_ID:-}" ] || return 0
  cd "${CT_REAL_WORKTREE:-/}" 2>/dev/null || cd /
  MSYS2_ARG_CONV_EXCL='*' subst "$CT_STABLE_BUILD_DRIVE_MAPPED:" /D >/dev/null 2>&1 || true
  CT_STABLE_BUILD_DRIVE_MAPPED=""
}

enter_build_gateway() {
  ct_gateway_acquire "$*"
  local gateway_log
  CT_BUILD_ARTIFACT_DIR="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.artifacts"
  mkdir -p "$CT_BUILD_ARTIFACT_DIR"
  export CT_BUILD_ARTIFACT_DIR
  gateway_log="$(ct_gateway_log_path)"
  echo "Build gateway job $CT_BUILD_JOB_ID log: $gateway_log"
  [ "$CMD" = check-queue ] || [ "$CMD" = check-workspace ] || enter_stable_build_path
  # Runs after the stable mapping so retention sees X: and refreshes the recorded gateway build directory.
  if [ "$CMD" != gc ]; then
    ct_cache_retention auto || echo "[cache-policy] retention refused; build continues" >&2
  fi
  if [ "${CT_BUILD_GATEWAY_NO_TEE:-0}" = "1" ] && [ "${CODETERM_BUILD_FINGERPRINTS:-0}" != 1 ]; then
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
  if [ "${CODETERM_BUILD_FINGERPRINTS:-0}" = 1 ]; then
    CT_BUILD_GATEWAY_TRACE_PATH="${gateway_log%.log}.fingerprint.log"
    CT_BUILD_GATEWAY_TRACE_FIFO="${gateway_log}.trace.fifo.$$"
    mkfifo "$CT_BUILD_GATEWAY_TRACE_FIFO" || exit "$CT_BUILD_GATEWAY_LOGGING_FAILURE"
    node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-output.mjs" filter "$CT_BUILD_GATEWAY_TRACE_PATH" < "$CT_BUILD_GATEWAY_TRACE_FIFO" &
    CT_BUILD_GATEWAY_TRACE_READER_PID=$!
    exec 2> "$CT_BUILD_GATEWAY_TRACE_FIFO"
  fi
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
  [ "$reader_success_status" -eq 0 ] || {
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
  [ "$reader_failure_status" -eq 101 ] || {
    echo "build gateway logging regression: reader failure after command failure exited $reader_failure_status" >&2
    return 1
  }
  grep -q 'build gateway logging path broke.*command exit was 101' "$root/reader-failure.console" || {
    echo "build gateway logging regression: failed command status was not reported" >&2
    return 1
  }
  echo "reader failures: command exits 0 and 101 preserved with logging degradation"
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
    if [ "$stuck_mode" = success ]; then [ "$stuck_status" -eq 0 ]; else [ "$stuck_status" -eq 101 ]; fi || return 1
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
  "reader-success": ["logging_degraded", 0, 0],
  "reader-failure": ["logging_degraded", 101, 101],
  "open-failure": ["logging_failed", 74, null],
  "command-killed": ["cancelled", 130, null],
  "stuck-success": ["logging_degraded", 0, 0],
  "stuck-command-fail": ["logging_degraded", 101, 101],
};
for (const [id, values] of Object.entries(expected)) {
  const job = jobs[id];
  if (!job) throw new Error(`gateway status omitted ${id}`);
  const actual = [job.outcome, job.exit_code, job.command_exit_code];
  if (job.outcome === 'logging_degraded' && job.state !== (job.exit_code === 0 ? 'completed' : 'failed')) {
    throw new Error(`${id} logging degradation changed the command state`);
  }
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
  dir="$(ct_cache_home)"
  identity="$(printf '%s' "${CT_REAL_WORKTREE:-$(pwd -P)}" | git hash-object --stdin)"
  export CARGO_TARGET_DIR="$(ct_exceptional_target_dir "${CT_REAL_WORKTREE:-$(pwd -P)}" "$dir" "$identity")"
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
  dir="$(ct_cache_home)/targets/release/$CT_BUILD_JOB_ID"
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
  dir="$(ct_cache_home)/targets/local-install/$identity"
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
    LOCAL_INSTALL_CONTINUE_PROMPT="Local Domios install completed for build $source_sha. Verify the production health endpoint reports this SHA, then continue the work that requested the reinstall."
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
cli_profile_dir() {
  local profile="$1" target_root="${CARGO_TARGET_DIR:-src-tauri/target}"
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "$target_root/$CARGO_BUILD_TARGET/$profile"
  else
    echo "$target_root/$profile"
  fi
}

cli_release_dir() {
  cli_profile_dir release
}

cli_profile_binary() {
  local release_dir
  release_dir="$(cli_profile_dir "$1")"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s/codeterm\n' "$release_dir"
  else
    printf '%s/codeterm.exe\n' "$release_dir"
  fi
}

cli_release_binary() {
  cli_profile_binary release
}

# The app binary is domios-app and the CLI's is codeterm, so the CLI shares the app's target directory.
dev_cli_target_dir() {
  printf '%s\n' "${CARGO_TARGET_DIR:-src-tauri/target}"
}

build_dev_cli() {
  local cli_target_dir
  cli_target_dir="$(dev_cli_target_dir)"
  dev_build_phase cargo-cli.begin
  CARGO_TARGET_DIR="$cli_target_dir" cargo build --manifest-path cli/Cargo.toml \
    --message-format=json-render-diagnostics > "$1"
  dev_build_phase cargo-cli.end
  built_cli_artifact="$(CARGO_TARGET_DIR="$cli_target_dir" cli_profile_binary debug)"
  built_cli_hash="$(cli_file_sha256_or_status "$built_cli_artifact")"
  dev_build_phase cli-hash.end
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

dev_resource_dir() {
  if [[ "$OSTYPE" == darwin* ]] && [ -n "${CODETERM_DEV_INSTANCE:-}" ]; then
    printf '%s\n' "$(tauri_debug_dir)/.domios-dev-runtime/Domios.app/Contents/Resources"
  elif [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s\n' "$(tauri_debug_dir)/.domios-dev-runtime/lib/codeterm"
  else
    tauri_debug_dir
  fi
}

dev_frontend_dir() {
  if [[ "$OSTYPE" == darwin* ]] && [ -n "${CODETERM_DEV_INSTANCE:-}" ]; then
    printf '%s\n' "$(pwd -P)/dist"
  else
    printf '%s\n' "$(dev_resource_dir)/dist"
  fi
}

dev_built_app_path() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s\n' "$(tauri_debug_dir)/domios-app"
  else
    printf '%s\n' "$(tauri_debug_dir)/domios-app.exe"
  fi
}

dev_app_path() {
  if [[ "$OSTYPE" == darwin* ]] && [ -n "${CODETERM_DEV_INSTANCE:-}" ]; then
    printf '%s\n' "$(tauri_debug_dir)/.domios-dev-runtime/Domios.app/Contents/MacOS/domios-dev-app"
  elif [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    printf '%s\n' "$(tauri_debug_dir)/.domios-dev-runtime/bin/domios-dev-app"
  else
    printf '%s\n' "$(tauri_debug_dir)/domios-dev-app.exe"
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
# pruned to the newest few. The copy keeps the name domios_app.pdb because Windows
# symbol lookup finds a PDB by the name recorded in the binary's RSDS record — a
# renamed file is never found, which is why the sha is the directory.
DEV_PDB_KEEP_DEFAULT=3

dev_pdb_store() {
  printf '%s
' "${CODETERM_DEV_PDB_DIR:-$CT_DATA_DIR/pdb}"
}

dev_pdb_source() {
  printf '%s
' "${CARGO_TARGET_DIR:-$(pwd -P)/src-tauri/target}/debug/domios_app.pdb"
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
  if [ ! -f "$dest/domios_app.pdb" ] || [ "$src" -nt "$dest/domios_app.pdb" ]; then
    cp -f "$src" "$dest/domios_app.pdb.partial"
    mv -f "$dest/domios_app.pdb.partial" "$dest/domios_app.pdb"
    echo "[symbols] retained $key -> $dest/domios_app.pdb"
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
    --launch-sha "$CODETERM_BUILD_SHA" \
    --record "$(dev_provenance_record)" \
    --build-sha "$build_sha" \
    --app "$(dev_app_path)" \
    --cli "$(dev_cli_path)" \
    --frontend "$(dev_frontend_dir)" \
    --instance "$CT_INSTANCE" \
    --api-port "$CT_PORT" \
    --vite-port "$CT_VITE_PORT" \
    --cdp-port "$CT_CDP_PORT" \
    --identifier "$CT_IDENTIFIER"
}

validate_dev_provenance() {
  local launch_args=()
  if [ -n "${CT_DEV_PROVENANCE_SHA:-}" ]; then
    launch_args=(--launch-sha "$CT_DEV_PROVENANCE_SHA")
  fi
  node scripts/dev-prep.mjs "${CT_DEV_PROVENANCE_OPERATION:---check-provenance}" \
    "${launch_args[@]}" \
    --record "$(dev_provenance_record)" \
    --app "$(dev_app_path)" \
    --cli "$(dev_cli_path)" \
    --frontend "$(dev_frontend_dir)" \
    --instance "$CT_INSTANCE" \
    --api-port "$CT_PORT" \
    --vite-port "$CT_VITE_PORT" \
    --cdp-port "$CT_CDP_PORT" \
    --identifier "$CT_IDENTIFIER"
}

build_dev_server() {
  dev_build_phase cargo-server.begin
  cargo build --manifest-path src-tauri/Cargo.toml -p domios-server --bin domios-server \
    --profile "$CT_DEV_CARGO_PROFILE" --no-default-features --features domios-server/embeddings,domios-server/mem-postgres \
    --message-format=json-render-diagnostics > "$1"
  dev_build_phase cargo-server.end
}

reload_dev_server() {
  if [[ "$OSTYPE" == darwin* ]] && [ -n "${CODETERM_DEV_INSTANCE:-}" ]; then
    echo 'Named macOS DEV uses a sealed app pair; run ./build.sh dev instead.' >&2
    return 2
  fi
  local plan split_logs receipt build_sha started
  started="$(date +%s)"
  CT_SPLIT_DEV=1
  if named_dev_isolated; then prepare_named_dev_target; fi
  plan="$(CT_DEV_PROVENANCE_OPERATION=--check-server-reload validate_dev_provenance)"
  split_logs="$(tauri_debug_dir)/.domios-dev-build"
  receipt="$split_logs/reload-server.json"
  node scripts/dev-server.mjs inspect "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$receipt"
  enter_cargo_gateway "$CMD"
  ct_configure_dev_qa_project
  export CODETERM_DEV=1
  configure_dev_cargo
  if [[ "$plan" == cli:* ]]; then
    build_dev_cli "$split_logs/cli.jsonl"
  else
    built_cli_artifact="$(dev_cli_path)"
    built_cli_hash="$(cli_file_sha256_or_status "$built_cli_artifact")"
  fi
  export CODETERM_SERVER_CLI_SHA256="$built_cli_hash"
  ct_with_firebase_build_config build_dev_server "$split_logs/server.jsonl"
  cargo tree --manifest-path src-tauri/Cargo.toml -p domios-server --no-default-features \
    --features domios-server/embeddings,domios-server/mem-postgres --edges normal,build --prefix none > "$split_logs/server-tree.txt"
  # Refuse edits made during compilation before touching the running instance.
  [ "$plan" = "$(CT_DEV_PROVENANCE_OPERATION=--check-server-reload validate_dev_provenance)" ] || {
    echo 'Build inputs changed during compilation; rerun ./build.sh dev-server.' >&2
    return 2
  }
  node scripts/server-artifact.mjs check-dev-server "$split_logs/server.jsonl" "$split_logs/server-tree.txt" \
    "$split_logs/cli.jsonl" "$(pwd -P)" "$(dev_resource_dir)" "$(dev_built_app_path)" "$CODETERM_BUILD_SHA"
  node scripts/dev-server.mjs stop "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$receipt"
  if [[ "$plan" == cli:* ]]; then install_cli_to "$CT_DATA_DIR/bin" "$built_cli_artifact" "$built_cli_hash"; fi
  build_sha="$CODETERM_BUILD_SHA"
  node scripts/server-artifact.mjs stage-dev-server "$split_logs/server.jsonl" "$split_logs/server-tree.txt" \
    "$split_logs/cli.jsonl" "$(pwd -P)" "$(dev_resource_dir)" "$(dev_built_app_path)" "$build_sha"
  write_dev_provenance "$build_sha"
  CT_DEV_PROVENANCE_SHA="$build_sha" validate_dev_provenance
  release_stable_build_path
  complete_build_gateway_before_handoff
  export CODETERM_CLI_PATH="$(dev_cli_path)"
  node scripts/dev-server.mjs start "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT"
  echo "DEV server reload completed in $(( $(date +%s) - started ))s; native client/window retained."
}

run_prebuilt_preflight() {
  local status=0 output
  output="$("$@" 2>&1)" || status=$?
  if [ -n "$output" ]; then
    printf '%s\n' "$output" >> "$PREBUILT_LOG"
    if [ "$status" = 0 ]; then printf '%s\n' "$output"; else printf '%s\n' "$output" >&2; fi
  fi
  return "$status"
}

refuse_prebuilt() {
  printf '%s\n' "$@" | tee -a "$PREBUILT_LOG" >&2
  exit 2
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
  local source="${2:-}" expected_hash="${3:-}" destination alias source_hash destination_hash
  if [ -z "$source" ]; then
    source="$(cli_release_binary)"
  fi
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    destination="$bin_dir/codeterm"
    alias="$bin_dir/domios"
  else
    destination="$bin_dir/codeterm.exe"
    alias="$bin_dir/domios.exe"
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
  destination_hash="$(cli_file_sha256_or_status "$alias")"
  if [ "$destination_hash" != "$expected_hash" ]; then
    if ! rm -f "$alias" || ! cp "$destination" "$alias"; then
      report_cli_install_failure "primary command copy did not succeed" \
        "$source" "$alias" "$expected_hash" "$(cli_file_sha256_or_status "$alias")"
      return 1
    fi
  fi
  destination_hash="$(cli_file_sha256_or_status "$alias")"
  if [ "$destination_hash" != "$expected_hash" ]; then
    report_cli_install_failure "primary command does not match the built artifact" \
      "$source" "$alias" "$expected_hash" "$destination_hash"
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
  node scripts/prepare-test-bundle.mjs "${CARGO_BUILD_TARGET:-$(ct_host_triple)}"
}

prepare_nextest_metadata() {
  node "${CT_GATEWAY_SCRIPT_ROOT:-.}/scripts/nextest-metadata-cache.mjs" "$@"
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
  awk -v max="$1" -v status="${2:-0}" '
    /^[^ ].*:[0-9]+:[0-9]+: error(\[E[0-9]+\])?:/ || /^(sccache: )?error(\[E[0-9]+\])?:/ && !/could not compile/ {
      total++
      if (total <= max) print
    }
    END {
      if (status != 0 && total == 0) {
        total=1
        if (max > 0) printf "error: cargo check failed (exit %d); see full log\n", status
      }
      printf "errors=%d shown=%d\n", total, (total < max ? total : max)
    }
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

configure_app_test_cargo() {
  configure_dev_cargo || return $?
  CT_APP_TEST_MANIFEST=src-tauri/Cargo.toml
  CT_APP_TEST_PACKAGE=codeterm-service-core
  CT_APP_TEST_TARGET_ARGS=(--lib --no-default-features)
  case "${1:?app test configuration requires a runner}" in
    nextest|test-one) ;;
    *) echo "Unknown app test runner: $1" >&2; return 2 ;;
  esac
  CT_APP_TEST_ARGS=(--manifest-path "$CT_APP_TEST_MANIFEST" -p "$CT_APP_TEST_PACKAGE" "${CT_APP_TEST_TARGET_ARGS[@]}")
}

configure_dev_cargo() {
  CT_DEV_CARGO_PROFILE=dev
  CT_DEV_CARGO_FEATURES="$(node "$CT_GATEWAY_SCRIPT_ROOT/scripts/dev-build-configuration.mjs" --guard src-tauri/crates/domios-server/Cargo.toml src-tauri/crates/codeterm-service-core/Cargo.toml)" || return $?
  # The native client carries no server features; CT_DEV_CARGO_FEATURES applies to server packages only.
  CT_DEV_CHECK_ARGS=(--manifest-path src-tauri/Cargo.toml -p codeterm --lib --profile "$CT_DEV_CARGO_PROFILE" --no-default-features)
  CT_DEV_TAURI_ARGS=(-- --profile "$CT_DEV_CARGO_PROFILE")
}

configure_product_cargo() {
  local package feature features=()
  CT_PRODUCT_CARGO_ARGS=(--no-default-features)
  IFS=, read -r -a features <<< "$CT_DEV_CARGO_FEATURES"
  for package in domios-server codeterm-service-core codeterm-mem; do
    case " $* " in
      *" --workspace "*|*" -p $package "*)
        for feature in "${features[@]}"; do CT_PRODUCT_CARGO_ARGS+=(--features "$package/$feature"); done
        ;;
    esac
  done
}

check_queue_worktree() {
  node "$CT_GATEWAY_SCRIPT_ROOT/scripts/check-worktree.mjs" \
    "$CT_GATEWAY_SCRIPT_ROOT" "$(ct_gateway_dir)" "$1" "$CT_BUILD_JOB_ID" "${CT_GATEWAY_OWNER_PID:-$$}" "$CT_GATEWAY_PROCESS_START"
}

enter_check_worktree() {
  enter_cargo_gateway "$CMD $1"
  CT_CHECK_WORKTREE="$(check_queue_worktree "$1")"
  cd "$CT_CHECK_WORKTREE"
  enter_stable_build_path "$CT_CHECK_WORKTREE"
  prepare_dev_frontend_dist "$CT_CHECK_WORKTREE"
  prepare_bundle_stubs
  unset CARGO_INCREMENTAL
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
}

# Whole-workspace cargo check of one SHA; a Linux peer runs it so cfg(unix) paths compile once per batch.
check_workspace_run() {
  [ "$#" -le 1 ] || { echo "Usage: ./build.sh check-workspace [<sha>]" >&2; exit 2; }
  local sha started status=0 out
  sha="$(git rev-parse --verify "${1:-HEAD}^{commit}")"
  CT_BUILD_SOURCE_SHA="$sha"
  CT_BUILD_CRATES="workspace codeterm-cli"
  enter_check_worktree "$sha"
  started="$(date +%s)"
  out="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.check-workspace.log"
  : > "$out"
  CARGO_TARGET_DIR="$CT_CHECK_WORKTREE/target/app" cargo check --manifest-path src-tauri/Cargo.toml \
    --workspace --all-targets --message-format=short >> "$out" 2>&1 || status=$?
  check_queue_errors 20 "$status" < "$out"
  check_queue_warnings 40 "*" < "$out" || [ "$status" != 0 ] || status=1
  echo "check-workspace: sha=$sha os=$OSTYPE exit=$status wall=$(( $(date +%s) - started ))s full-log=$out"
  return "$status"
}

# User scope only: --no-modify-path leaves the machine PATH alone; remove with `rustup self uninstall` and deleting cargo-nextest.
peer_toolchain_install() {
  local want bin="${CARGO_HOME:-$HOME/.cargo}/bin" tmp nextest dl=(curl --proto '=https' --tlsv1.2 -fsSL --speed-limit 1000 --speed-time 60)
  want="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
  tmp="$(mktemp -d)"
  if ! command -v rustup >/dev/null 2>&1; then
    case "$OSTYPE" in
      darwin*|linux*) "${dl[@]}" https://sh.rustup.rs -o "$tmp/rustup-init.sh" \
        && sh "$tmp/rustup-init.sh" -y --no-modify-path --profile minimal --default-toolchain "$want" ;;
      *) "${dl[@]}" https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe -o "$tmp/rustup-init.exe" \
        && "$tmp/rustup-init.exe" -y --no-modify-path --profile minimal --default-toolchain "$want" ;;
    esac || { rm -rf "$tmp"; echo "peer-toolchain: rustup install failed" >&2; return 1; }
  fi
  rm -rf "$tmp"
  export PATH="$bin:$PATH"
  rustup toolchain install "$want" --profile minimal --component clippy,rustfmt || return 1
  if ! command -v cargo-nextest >/dev/null 2>&1; then
    case "$OSTYPE" in
      darwin*) nextest=mac ;;
      linux*) [ "$(uname -m)" = aarch64 ] && nextest=linux-arm || nextest=linux ;;
      *) nextest=windows-tar ;;
    esac
    "${dl[@]}" "https://get.nexte.st/latest/$nextest" | tar zxf - -C "$bin" || { echo "peer-toolchain: cargo-nextest install failed" >&2; return 1; }
  fi
}

peer_toolchain() {
  local want rustc_v rustup_v native ok=1 tool
  if [ "${1:-}" = --install ]; then peer_toolchain_install || return 1; fi
  want="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
  rustup_v="$(rustup --version 2>/dev/null | awk 'NR==1{print $2}')"
  rustc_v="$(rustc --version 2>/dev/null | awk '{print $2}')"
  [ -n "$rustup_v" ] && [ "$rustc_v" = "$want" ] || ok=0
  case "$OSTYPE" in
    darwin*) native="xcode-clt=$(xcode-select -p >/dev/null 2>&1 && echo ok || echo missing)" ;;
    linux*) native="cc=$(command -v cc >/dev/null 2>&1 && echo ok || echo missing)" ;;
    *) native="msvc=$( (setup_env) >/dev/null 2>&1 && echo ok || echo missing)" ;;
  esac
  [ "${native#*=}" = ok ] || ok=0
  for tool in node cargo-nextest sccache; do
    command -v "$tool" >/dev/null 2>&1 || { native="$native $tool=missing"; ok=0; }
  done
  echo "peer-toolchain: ok=$ok rustup=${rustup_v:-missing} rustc=${rustc_v:-missing} want=$want $native"
  [ "$ok" = 1 ]
}

peer_dispatch() {
  exec node scripts/build-peer.mjs "$@"
}

run_check_queue_cargo() {
  local output="$1" status=0 pipeline
  shift
  if [ "${CODETERM_BUILD_FINGERPRINTS:-0}" = 1 ]; then
    if "$@" 2>&1 | node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-output.mjs" filter "$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.fingerprint.log" >> "$output"; then
      return 0
    else
      pipeline=("${PIPESTATUS[@]}")
      status="${pipeline[0]}"
      [ "${pipeline[1]}" = 0 ] || { CT_BUILD_GATEWAY_TRACE_DEGRADED=1; echo 'WARNING: check-queue trace collection degraded; command exit preserved' >&2; }
      return "$status"
    fi
  fi
  "$@" >> "$output" 2>&1
}

check_queue_run() {
  local sha="" max_errors=20 max_warnings=40 touched_only=0 all=0 lane=0 arg base cw files targets pkg_args=() cli=0 started status=0 out touched="" changes
  while [ "$#" -gt 0 ]; do
    arg="$1"
    case "$arg" in
      --max-errors) max_errors="${2:?--max-errors requires a count}"; shift ;;
      --max-warnings) max_warnings="${2:?--max-warnings requires a count}"; shift ;;
      --touched-only) touched_only=1 ;;
      --all) all=1 ;;
      --lane) lane=1; touched_only=1 ;;
      -*) echo "Usage: ./build.sh check-queue [<sha>] [--max-errors N] [--max-warnings N] [--touched-only] [--all] [--lane]" >&2; exit 2 ;;
      *) sha="$arg" ;;
    esac
    shift
  done
  sha="$(git rev-parse --verify "${sha:-HEAD}^{commit}")"
  [ "$lane:$all" != 1:1 ] || { echo 'ERROR: --lane and --all select different verification scopes' >&2; return 2; }
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
    pkg_args=(--workspace --exclude codeterm-cli)
    cli=1
    touched="*"
  else
    while read -r kind pkg; do
      if [ "$kind" = cli ]; then cli=1; touched="${touched:-} codeterm-cli"; else pkg_args+=(-p "$pkg"); touched="${touched:-} $pkg"; fi
    done <<< "$targets"
  fi
  if [ "${#pkg_args[@]}" -gt 0 ] && [ "$all" = 0 ] && [ "$touched_only" = 0 ] && ! printf '%s\n' "${pkg_args[@]}" | grep -qx codeterm; then
    pkg_args+=(-p codeterm)
    targets="$(printf '%s\napp codeterm' "$targets")"
  fi
  source scripts/clippy-allow.sh

  CT_BUILD_SOURCE_SHA="$sha"
  CT_BUILD_CRATES="$(printf '%s\n' "${pkg_args[@]}" | sed '/^-p$/d')"
  [ "$cli" != 1 ] || CT_BUILD_CRATES="${CT_BUILD_CRATES}${CT_BUILD_CRATES:+ }codeterm-cli"
  CT_BUILD_TOUCHED_FILES_COUNT="$(printf '%s\n' "$files" | sed '/^$/d' | wc -l | tr -d ' ')"
  enter_check_worktree "$sha"
  cw="$CT_CHECK_WORKTREE"
  started="$(date +%s)"
  out="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.check.log"
  : > "$out"
  local plan="$out.plan" metadata="$out.metadata" package tests bins lib lane_args=() test_packages=()
  printf '%s\n' "$targets" > "$out.targets"
  if [ -n "$base" ]; then git diff --name-status --find-renames "$base" "$sha" > "$out.changes"; else : > "$out.changes"; fi
  cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 > "$metadata"
  node "$CT_GATEWAY_SCRIPT_ROOT/scripts/check-queue-plan.mjs" "$metadata" "$out.targets" "$out.changes" > "$plan"
  if [ "$all" = 0 ]; then
    pkg_args=()
    touched=""
    while read -r package tests bins lib; do pkg_args+=(-p "$package"); touched="$touched $package"; done < "$plan"
    [ "$cli" != 1 ] || touched="$touched codeterm-cli"
  fi
  configure_dev_cargo
  if [ "$lane" = 1 ]; then
    while read -r package tests bins lib; do
      [ "$lib" = 1 ] || continue
      if [ "$tests" = 1 ]; then test_packages+=("$package"); fi
      lane_args+=(-p "$package")
    done < "$plan"
    if grep -q '^codeterm[[:space:]]0' "$plan"; then lane_args+=(--bins); fi
    if [ "${#lane_args[@]}" -gt 0 ]; then
      configure_product_cargo "${lane_args[@]}"
      CARGO_TARGET_DIR="$cw/target/app" run_check_queue_cargo "$out" cargo clippy --manifest-path src-tauri/Cargo.toml "${lane_args[@]}" "${CT_PRODUCT_CARGO_ARGS[@]}" --lib --no-deps --message-format short -- -D warnings "${CLIPPY_ALLOW[@]}" || status=$?
    fi
    for package in "${test_packages[@]}"; do
      local test_args=(--lib --tests)
      [ "$package" != codeterm ] || test_args+=(--bins)
      CARGO_TARGET_DIR="$cw/target/app" run_check_queue_cargo "$out" cargo clippy --manifest-path src-tauri/Cargo.toml -p "$package" "${test_args[@]}" --no-default-features --profile test --no-deps --message-format short -- -D warnings "${CLIPPY_ALLOW[@]}" || status=$?
    done
  elif [ "${#pkg_args[@]}" -gt 0 ]; then
    configure_product_cargo "${pkg_args[@]}"
    CARGO_TARGET_DIR="$cw/target/app" run_check_queue_cargo "$out" cargo clippy --manifest-path src-tauri/Cargo.toml \
      "${pkg_args[@]}" "${CT_PRODUCT_CARGO_ARGS[@]}" --lib --bins --no-deps --message-format=short -- "${CLIPPY_ALLOW[@]}" || status=$?
  fi
  if [ "$cli" = 1 ]; then
    local cli_targets=(--all-targets)
    [ "$lane" != 1 ] || cli_targets=(-p codeterm-cli --bins)
    CARGO_TARGET_DIR="$cw/target/app" run_check_queue_cargo "$out" cargo clippy --manifest-path cli/Cargo.toml \
      "${cli_targets[@]}" --no-deps --message-format=short -- -D warnings "${CLIPPY_ALLOW[@]}" || status=$?
  fi
  if [ "$lane" = 0 ]; then
  local test_runner test_status
  for test_runner in nextest; do
    configure_app_test_cargo "$test_runner"
    test_status=0
    CARGO_TARGET_DIR="$cw/target/app" run_check_queue_cargo "$out" cargo check "${CT_APP_TEST_ARGS[@]}" --profile test --tests --message-format=short || test_status=$?
    [ "$test_status" = 0 ] || status="$test_status"
    echo "check-queue: app-test scope=$test_runner,test-one args=${CT_APP_TEST_ARGS[*]} --profile test --tests exit=$test_status" | tee -a "$out"
  done
  fi
  if [ "${CODETERM_BUILD_FINGERPRINTS:-0}" = 1 ]; then
    node "$CT_GATEWAY_SCRIPT_ROOT/scripts/build-gateway-output.mjs" fingerprint-summary "$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.fingerprint.log"
  fi
  check_queue_errors "$max_errors" "$status" < "$out"
  check_queue_warnings "$max_warnings" "$touched" < "$out" || [ "$status" != 0 ] || status=1
  echo "check-queue: sha=$sha lane=$lane packages=${pkg_args[*]:-} cli=$cli exit=$status wall=$(( $(date +%s) - started ))s full-log=$out"
  return "$status"
}

# Instance-specific settings are applied to the Tauri context at startup.
dev_conf_path() {
  local base="src-tauri/tauri.dev.conf.json"
  local out="src-tauri/tauri.dev.conf-runtime.json"
  node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('$base','utf8'));c.build=c.build||{};c.build.beforeDevCommand=null;const bytes=JSON.stringify(c,null,2);if(!fs.existsSync('$out')||fs.readFileSync('$out','utf8')!==bytes)fs.writeFileSync('$out',bytes);" >&2
  echo "$out"
}

# A Team dev tab lets processes leave its kill-on-close job; the DEV window must, so restarting the tab keeps it.
run_outside_tab_job() {
  local background_record="" background_log=""
  if [ "${1:-}" = --background ]; then
    background_record="$2" background_log="$3"
    shift 3
  fi
  if [[ "$OSTYPE" != msys* && "$OSTYPE" != cygwin* ]]; then
    "$@"
    return
  fi
  local log status=0 line="" arg
  # MSYS converts POSIX paths only for programs bash starts itself, not inside a raw command line.
  for arg in "$@"; do
    [[ "$arg" == /* ]] && arg="$(cygpath -w "$arg")"
    line+="\"$arg\" "
  done
  local -a background_args=()
  if [ -n "$background_record" ]; then
    log="$background_log"
    background_args=(-ProcessRecord "$(cygpath -w "$background_record")")
  else
    log="$(mktemp)"
  fi
  BREAKAWAY_COMMAND_LINE="$line" powershell.exe -NoProfile -ExecutionPolicy Bypass \
    -File "$(cygpath -w scripts/windows-breakaway-run.ps1)" -Log "$(cygpath -w "$log")" ${background_args[@]+"${background_args[@]}"} || status=$?
  if [ -z "$background_record" ]; then
    cat "$log"
    rm -f "$log"
  fi
  if [ "$status" = 250 ]; then
    if [ -n "$background_record" ]; then
      echo "ERROR: this tab's job refuses detached DEV frontend launch; no contained fallback." >&2
      return 250
    fi
    echo "This tab's job refuses breakaway; the DEV window stays tied to this tab."
    "$@"
    return
  fi
  return "$status"
}

# The live DEV window's executable when it runs from this instance's recorded target directory.
dev_live_window_exe() {
  [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]] || return 1
  local target_win
  target_win="$(cat "$(dev_target_record)" 2>/dev/null || true)"
  [ -n "$target_win" ] || return 1
  powershell.exe -NoProfile -Command "\$dir = [IO.Path]::GetFullPath('$target_win').TrimEnd('\\'); Get-Process domios-dev-app -ErrorAction SilentlyContinue | Where-Object { \$_.Path -and [IO.Path]::GetDirectoryName(\$_.Path) -ieq \$dir } | Select-Object -First 1 -ExpandProperty Path" 2>/dev/null | tr -d '\r' | grep -m1 .
}

# Only the split dev-back owns a window it can keep; Windows proves an unchanged client through its exe lock.
dev_window_retainable() {
  [ "${DEV_FRONT_EXTERNAL:-0}" = 1 ] && dev_live_window_exe >/dev/null
}

# The running window locks its executable, so a build that succeeds beside it left the client unchanged.
build_dev_client() {
  local conf="$1" log
  shift
  if [ "${CT_DEV_RETAIN_WINDOW:-0}" != 1 ]; then
    TAURI_CONFIG="$(cat "$conf")" cargo build "$@"
    return
  fi
  log="$(tauri_debug_dir)/.domios-dev-build/client.log"
  if TAURI_CONFIG="$(cat "$conf")" cargo build "$@" 2>&1 | tee "$log"; [ "${PIPESTATUS[0]}" = 0 ]; then
    return 0
  fi
  grep -qE "LNK1104|failed to (link or copy|remove)|os error (5|32)|Access is denied" "$log" || return 1
  echo "Native client changed; replacing the DEV window."
  CT_DEV_RETAIN_WINDOW=0
  stop_dev_before_replacement || return
  TAURI_CONFIG="$(cat "$conf")" cargo build "$@"
}

# Swaps only the server under a retained window, which shows Reconnecting and reattaches.
swap_dev_server_keep_window() {
  local split_logs="$1" build_sha="$2" receipt="$1/reload-server.json" listener
  if node scripts/dev-server.mjs inspect "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$receipt"; then
    node scripts/dev-server.mjs stop "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$receipt"
  fi
  node scripts/server-artifact.mjs stage-dev "$split_logs/server.jsonl" "$split_logs/server-tree.txt" \
    "$split_logs/cli.jsonl" "$(pwd -P)" "$(dev_resource_dir)" "$(dev_built_app_path)" "$build_sha"
  write_dev_provenance "$build_sha"
  retain_dev_pdb "$build_sha"
  release_stable_build_path
  record_dev_target "$(tauri_debug_dir)"
  complete_build_gateway_before_handoff
  export CODETERM_CLI_PATH="$(dev_cli_path)"
  # The retained window relaunches its server on detachment, so a refused start may only mean it won the race.
  if ! node scripts/dev-server.mjs start "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT"; then
    node scripts/dev-server.mjs inspect "$(dev_provenance_record)" "$CT_DATA_DIR" "$CT_PORT" "$receipt"
    echo "The retained window already relaunched the recorded DEV server build."
  fi
  listener="$(dev_listener_pid)"
  echo "DEV server swapped; native client/window retained."
  if [ -n "$DEV_NOTIFY_TAB" ]; then
    dev_notify_ready_after_proof "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CT_INSTANCE" "$CT_PORT" "$build_sha" "$listener" "$(pwd -P)"
  fi
}

build_and_launch_dev() {
  local conf build_sha
  export CODETERM_DEV=1
  export CODETERM_WEB_DIST="$(pwd -P)/dist"
  configure_dev_cargo
  conf="$(dev_conf_path)"
  dev_build_phase bundle-cli.begin
  node scripts/bundle-cli.js "$built_cli_artifact"
  dev_build_phase bundle-cli.end
  local split_logs="$(tauri_debug_dir)/.domios-dev-build"
  export CODETERM_SERVER_CLI_SHA256="$built_cli_hash"
  dev_build_phase firebase-server.begin
  ct_with_firebase_build_config build_dev_server "$split_logs/server.jsonl"
  dev_build_phase firebase-server.end
  cargo tree --manifest-path src-tauri/Cargo.toml -p domios-server --no-default-features \
    --features domios-server/embeddings,domios-server/mem-postgres --edges normal,build --prefix none > "$split_logs/server-tree.txt"
  dev_build_phase server-tree.end
  local build_args=(--manifest-path src-tauri/Cargo.toml -p codeterm --bin domios-app
    --profile "$CT_DEV_CARGO_PROFILE" --no-default-features)
  build_dev_client "$conf" "${build_args[@]}"
  dev_build_phase cargo-client.end
  build_sha="$CODETERM_BUILD_SHA"
  if [[ "$OSTYPE" == darwin* ]] && named_dev_isolated; then prepare_local_macos_signing 0; fi
  build_dev_frontend
  dev_build_phase frontend.end
  if [ "${CT_DEV_RETAIN_WINDOW:-0}" = 1 ]; then
    if dev_live_window_exe >/dev/null && cmp -s "$(dev_built_app_path)" "$(dev_app_path)"; then
      swap_dev_server_keep_window "$split_logs" "$build_sha"
      return
    fi
    stop_dev_before_replacement || return
  fi
  stop_dev_before_replacement || return
  dev_build_phase stop-before-stage.end
  node scripts/server-artifact.mjs stage-dev "$split_logs/server.jsonl" "$split_logs/server-tree.txt" \
    "$split_logs/cli.jsonl" "$(pwd -P)" "$(dev_resource_dir)" "$(dev_built_app_path)" "$build_sha"
  dev_build_phase stage.end
  write_dev_provenance "$build_sha"
  dev_build_phase provenance.end
  retain_dev_pdb "$build_sha"
  dev_build_phase symbols.end
  release_stable_build_path
  record_dev_target "$(tauri_debug_dir)"
  complete_build_gateway_before_handoff
  dev_build_phase gateway-release.end
  echo "Build gateway released; launching validated dev artifact."
  CT_DEV_HANDOFF_SHA="$CODETERM_BUILD_SHA" CT_DEV_HANDOFF_HASH="$CODETERM_BUILD_HASH" exec bash ./build.sh dev-prebuilt "$@"
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

  local root_win target_debug_win data_dir_win cleanup_script ps
  local -a cleanup_args
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
  if [ "$#" -gt 0 ]; then
    cleanup_args+=(-OwnedProcessRecords "$(cygpath -w "$1")" -OwnedOnly)
  fi
  if [ "${DEV_FRONT_EXTERNAL:-0}" = 1 ]; then
    cleanup_args+=(-KeepVite)
  fi
  "${cleanup_args[@]}" || true
}

# Provenance must hash the bytes the prebuilt watcher regenerates for this HEAD, not a prior session's dist/.
build_dev_frontend() {
  local log="/tmp/codeterm-web-watch${CT_SUFFIX}.log"
  echo "Building dev frontend bundle (log $log)..."
  if ! CODETERM_FRONTEND_DEV=1 node node_modules/vite/bin/vite.js build </dev/null >> "$log" 2>&1; then
    echo "ERROR: dev frontend bundle build failed; see $log" >&2
    return 1
  fi
}

# The static server serves dist/ during the watcher's first rebuild, so the watcher overwrites in place.
start_web_watch() {
  kill_web_watch || true
  if [ "${CODETERM_DEV_WATCH:-1}" = 0 ]; then
    echo "Web bundle watcher disabled; serving the provenance-validated frontend unchanged."
    return 0
  fi
  local log="/tmp/codeterm-web-watch${CT_SUFFIX}.log"
  local vite_entry="node_modules/vite/bin/vite.js"
  local pid shell_pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      CODETERM_FRONTEND_DEV=1 run_outside_tab_job --background "$(web_watch_pidfile)" "$log" \
        node "$(pwd -P)/$vite_entry" build --watch --emptyOutDir false --outDir "$(dev_frontend_dir)"
      spawn_status=$?
      set -e
      [ "$spawn_status" -eq 0 ] || return "$spawn_status"
      echo "Web bundle watcher started outside tab job (identity $(web_watch_pidfile), log $log)"
      return
      ;;
    *)
      CODETERM_FRONTEND_DEV=1 nohup node "$vite_entry" build --watch --emptyOutDir false --outDir "$(dev_frontend_dir)" </dev/null >> "$log" 2>&1 &
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
      CODETERM_DEV_STATIC_DIST="$(dev_frontend_dir)" run_outside_tab_job --background "$(dev_static_pidfile)" "$log" \
        node "$(pwd -P)/scripts/dev-static-server.mjs"
      spawn_status=$?
      set -e
      [ "$spawn_status" -eq 0 ] || return "$spawn_status"
      echo "Dev static server started outside tab job (devUrl port $CT_VITE_PORT, identity $(dev_static_pidfile), log $log)"
      return
      ;;
    *)
      CODETERM_DEV_STATIC_DIST="$(dev_frontend_dir)" nohup node scripts/dev-static-server.mjs </dev/null >> "$log" 2>&1 &
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
      windows_dev_leftover_cleanup "$pidfile"
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
      windows_dev_leftover_cleanup "$pidfile"
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
      process.stdout.write([json.status ?? "", json.build_sha ?? "", instance.name ?? "", instance.api_port ?? "", instance.vite_port ?? "", instance.cdp_port ?? "", instance.identifier ?? "", json.startup?.state ?? ""].join("\n"));
    });
  '
}

validate_dev_health() {
  local expected_sha="$1" expected_instance="$2" expected_port="$3" expected_vite="$4" expected_cdp="$5" expected_identifier="$6" health="$7"
  local role="${8:-combined}"
  case "$role" in combined|server) ;; *) echo "Unknown DEV health role" >&2; return 1 ;; esac
  local status sha instance port vite cdp identifier fields startup
  fields="$(printf '%s' "$health" | dev_health_fields 2>/dev/null || true)"
  status="$(printf '%s\n' "$fields" | sed -n '1p')"
  sha="$(printf '%s\n' "$fields" | sed -n '2p')"
  instance="$(printf '%s\n' "$fields" | sed -n '3p')"
  port="$(printf '%s\n' "$fields" | sed -n '4p')"
  vite="$(printf '%s\n' "$fields" | sed -n '5p')"
  cdp="$(printf '%s\n' "$fields" | sed -n '6p')"
  identifier="$(printf '%s\n' "$fields" | sed -n '7p')"
  startup="$(printf '%s\n' "$fields" | sed -n '8p')"
  if [ "$status" != "ok" ]; then
    echo "Health endpoint did not report status=ok" >&2
    return 1
  fi
  if [ "$startup" != "ready" ]; then
    echo "Health endpoint did not report startup.state=ready" >&2
    return 1
  fi
  if [[ ! "$expected_sha" =~ ^[0-9a-f]{40}$ ]] || [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]] || [ "$sha" != "$expected_sha" ]; then
    echo "Health endpoint is stale or wrong-lane: expected build $expected_sha, got $sha" >&2
    return 1
  fi
  if [ "$instance" != "$expected_instance" ] || [ "$port" != "$expected_port" ] ||
      { [ "$role" = combined ] && { [ "$vite" != "$expected_vite" ] || [ "$cdp" != "$expected_cdp" ] || [ "$identifier" != "$expected_identifier" ]; }; }; then
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
  local tab_id="$1" health sha role=combined listener
  [ -n "${EXPECTED_BUILD_SHA:-}" ] || return 1
  [ "${CT_SPLIT_DEV:-0}" != 1 ] || role=server
  health="$(dev_health_status)"
  sha="$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$health" "$role" 2>/dev/null)" || return 1
  listener="$(dev_listener_pid)"
  if [ "$role" = server ]; then
    node scripts/dev-prep.mjs --check-attachment --log "$DEV_NOTIFY_LOG" --api-port "$CT_PORT" --listener-pid "$listener" >/dev/null 2>&1 || return 1
  fi
  dev_notify_ready_after_proof "$DEV_NOTIFY_SENT_MARKER" "$tab_id" "$CT_INSTANCE" "$CT_PORT" "$sha" "$listener" "$(pwd -P)"
}

ensure_deps() {
  if [ ! -f node_modules/.modules.yaml ] || [ pnpm-lock.yaml -nt node_modules/.modules.yaml ]; then
    echo "Preparing checkout-local JS dependencies from the shared pnpm store (pnpm install --frozen-lockfile)..."
    CI=true pnpm install --frozen-lockfile
  fi
}

prepare_release_bundle_inputs() {
  local mode="${1:-standalone}"
  case "$mode" in
    standalone|bundle) ;;
    *) echo "Unknown release preparation mode: $mode" >&2; return 2 ;;
  esac
  local minimum_macos_version
  minimum_macos_version=$(node -p "require('./src-tauri/tauri.conf.json').bundle?.macOS?.minimumSystemVersion || ''")
  if [ -n "$minimum_macos_version" ]; then
    export MACOSX_DEPLOYMENT_TARGET="$minimum_macos_version"
  fi
  local cargo_target_args=()
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    cargo_target_args=(--target "$CARGO_BUILD_TARGET")
  fi
  # A release run builds the frontend once; its jobs install those bytes with a verified digest.
  if [ -n "${CODETERM_FRONTEND_ARCHIVE:-}" ]; then
    node scripts/release-frontend-dist.mjs install "$CODETERM_FRONTEND_ARCHIVE" "${CODETERM_FRONTEND_SHA256:-}" dist
  else
    pnpm build
  fi
  if [ "${CODETERM_RELEASE_COMMON_STAGE:-0}" = 1 ]; then
    node scripts/release-stage.mjs stage-build-resources --target "${CARGO_BUILD_TARGET:-$(ct_host_triple)}"
  fi
  cargo build --release --timings --manifest-path cli/Cargo.toml "${cargo_target_args[@]}"
  node scripts/bundle-cli.js
  if [ "${CODETERM_RELEASE_COMMON_STAGE:-0}" = 1 ]; then
    local release_target release_sha release_logs
    release_target="${CARGO_BUILD_TARGET:-$(ct_host_triple)}"
    if [[ "$release_target" == *-apple-darwin ]]; then
      : "${APPLE_SIGNING_IDENTITY:?Release CLI signing identity is required}"
      : "${RELEASE_KEYCHAIN:?Release keychain is required}"
      codesign --force --options runtime --timestamp --keychain "$RELEASE_KEYCHAIN" \
        --sign "$APPLE_SIGNING_IDENTITY" src-tauri/bundled-cli/codeterm
      codesign --verify --strict src-tauri/bundled-cli/codeterm
    fi
    release_sha="$(git rev-parse HEAD)"
    release_logs="release-artifacts/build-logs/$release_target"
    mkdir -p "$release_logs"
    CODETERM_SERVER_CLI_SHA256="$(node scripts/release-stage.mjs cli-digest --target "$release_target")"
    export CODETERM_SERVER_CLI_SHA256
    cargo build --release --manifest-path src-tauri/Cargo.toml -p domios-server --bin domios-server \
      --no-default-features --features domios-server/embeddings,domios-server/mem-postgres --target "$release_target" \
      --message-format=json > "$release_logs/server.jsonl"
    cargo tree --manifest-path src-tauri/Cargo.toml -p domios-server --target "$release_target" \
      --no-default-features --features domios-server/embeddings,domios-server/mem-postgres --edges normal,build --prefix none \
      --format '{p}' > "$release_logs/closure.txt"
    node scripts/release-stage.mjs prepare --target "$release_target" --source-sha "$release_sha" \
      --cargo-log "$release_logs/server.jsonl" --tree-log "$release_logs/closure.txt" \
      --cli-sha256 "$CODETERM_SERVER_CLI_SHA256"
  fi
  if [ "$mode" = "standalone" ]; then
    cargo build --release --timings --manifest-path src-tauri/Cargo.toml \
      "${cargo_target_args[@]}" \
      --features "tauri/custom-protocol${CODETERM_RELEASE_FEATURES:+,$CODETERM_RELEASE_FEATURES}" \
      --bin domios-app
  fi
  if [ "$mode" = "bundle" ] && [ "${CODETERM_RELEASE_COMMON_STAGE:-0}" != 1 ]; then
    node scripts/prepare-bundle-resource-stubs.mjs assert-package-ready
  fi
}

enter_cargo_gateway() {
  dev_build_phase gateway-enter.begin
  enter_build_gateway "$@"
  dev_build_phase gateway-enter.end
  ct_configure_sccache
  dev_build_phase cache-setup.end
}

package_platform() {
  case "$1" in
    *-pc-windows-*) echo windows ;;
    *-apple-darwin) echo macos ;;
    *-linux-*) echo linux ;;
    *) echo "Unsupported package target: $1" >&2; return 2 ;;
  esac
}

# Compiles the client once, replaces the compile stage with the final pair resources, then bundles from it.
# Arguments, when given, prefix each Tauri command (local builds add their build config).
bundle_common_stage() {
  local target="${CARGO_BUILD_TARGET:-$(ct_host_triple)}" client_dir="${CARGO_TARGET_DIR:-src-tauri/target}"
  local platform client=domios-app target_args=() compile_config=() unsigned_config=() run=("$@")
  platform="$(package_platform "$target")" || return
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    target_args=(--target "$CARGO_BUILD_TARGET")
    client_dir="$client_dir/$CARGO_BUILD_TARGET"
  fi
  # The raw Cargo server and sidecar stub would occupy the staged pair paths during compilation.
  case "$platform" in
    windows) client=domios-app.exe ;;
    macos) compile_config=(--config '{"bundle":{"externalBin":[]}}') ;;
    linux) compile_config=(--config '{"bundle":{"resources":[],"externalBin":[]}}') ;;
  esac
  if [ "${CODETERM_RELEASE_UNSIGNED:-0}" = 1 ]; then
    [ "$platform" != macos ] || { echo "Unsigned hosted compilation excludes macOS" >&2; return 2; }
    unsigned_config=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
  fi
  "${run[@]}" pnpm tauri build --no-bundle "${target_args[@]}" ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} "${compile_config[@]}" "${unsigned_config[@]}"
  node scripts/release-stage.mjs stage-full --target "$target" \
    --source-sha "$(git rev-parse HEAD)" \
    --cargo-log "release-artifacts/build-logs/$target/server.jsonl" \
    --tree-log "release-artifacts/build-logs/$target/closure.txt" \
    --client-file "$client_dir/release/$client"
  case "$platform" in
    windows) "${run[@]}" pnpm tauri bundle --no-binary-patching "${target_args[@]}" ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} --bundles nsis "${unsigned_config[@]}" ;;
    linux) "${run[@]}" pnpm tauri bundle --no-binary-patching "${target_args[@]}" ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} --bundles deb,appimage "${unsigned_config[@]}" ;;
    macos) "${run[@]}" node scripts/release-macos-bundle.mjs "$target" ;;
  esac
}

# Local macOS packages sign from the login keychain unless the caller supplies its own identity.
prepare_local_macos_signing() {
  local notarize="$1" identities count
  if [ -z "${RELEASE_KEYCHAIN:-}" ]; then
    RELEASE_KEYCHAIN="$(security login-keychain | sed -e 's/^[[:space:]]*"//' -e 's/"[[:space:]]*$//')"
    [ -n "$RELEASE_KEYCHAIN" ] || { echo "ERROR: no login keychain found for local macOS signing" >&2; return 2; }
    export RELEASE_KEYCHAIN
  fi
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    identities="$(security find-identity -v -p codesigning "$RELEASE_KEYCHAIN" |
      sed -n 's/^.*"\(Developer ID Application: .*\)"$/\1/p' | sort -u)"
    count="$(printf '%s' "$identities" | grep -c . || true)"
    if [ "$count" != 1 ]; then
      echo "ERROR: expected one Developer ID Application identity in $RELEASE_KEYCHAIN, found $count; set APPLE_SIGNING_IDENTITY" >&2
      return 2
    fi
    export APPLE_SIGNING_IDENTITY="$identities"
  fi
  if [ -z "${APPLE_TEAM_ID:-}" ]; then
    APPLE_TEAM_ID="$(printf '%s' "$APPLE_SIGNING_IDENTITY" | sed -n 's/^.*(\([A-Z0-9]\{10\}\))$/\1/p')"
    [ -n "$APPLE_TEAM_ID" ] || { echo "ERROR: signing identity names no team; set APPLE_TEAM_ID" >&2; return 2; }
    export APPLE_TEAM_ID
  fi
  echo "macOS signing: $APPLE_SIGNING_IDENTITY ($RELEASE_KEYCHAIN)"
  if [ "$notarize" = 0 ]; then
    echo "macOS notarization: skipped by --no-notarize"
    return 0
  fi
  export APPLE_KEYCHAIN_PROFILE="${APPLE_KEYCHAIN_PROFILE:-codeterm}"
  if ! xcrun notarytool history --keychain-profile "$APPLE_KEYCHAIN_PROFILE" --keychain "$RELEASE_KEYCHAIN" >/dev/null 2>&1; then
    echo "ERROR: notarytool keychain profile '$APPLE_KEYCHAIN_PROFILE' is unavailable in $RELEASE_KEYCHAIN" >&2
    echo "Store it with 'xcrun notarytool store-credentials $APPLE_KEYCHAIN_PROFILE', or pass --no-notarize." >&2
    return 2
  fi
  echo "macOS notarization: keychain profile $APPLE_KEYCHAIN_PROFILE"
}

notarize_local_macos_package() {
  local bundle_root="$1" dmgs=()
  [ -d "$bundle_root/dmg" ] && dmgs=("$bundle_root"/dmg/*.dmg)
  if [ "${#dmgs[@]}" -ne 1 ] || [ ! -f "${dmgs[0]}" ]; then
    echo "ERROR: expected one disk image under $bundle_root/dmg" >&2
    return 1
  fi
  bash .agents/skills/github-release/scripts/notarize-dmg.sh "${dmgs[0]}"
  bash scripts/staple-with-retry.sh "$bundle_root/macos/Domios.app"
  spctl --assess --type execute "$bundle_root/macos/Domios.app"
}

case "$CMD" in
  check-queue|nextest|test-one)
    for arg in "${@:2}"; do
      [ "$arg" != --peer ] || peer_dispatch dispatch "$CMD" "${@:2}" --state-dir "$(ct_gateway_dir)/peer-jobs"
    done
    ;;
  check-linux)
    peer_dispatch dispatch check-linux "${@:2}" --state-dir "$(ct_gateway_dir)/peer-jobs"
    ;;
  check-series)
    peer_dispatch dispatch check-series "${@:2}" --state-dir "$(ct_gateway_dir)/peer-jobs"
    ;;
  peer-setup)
    peer_dispatch setup "${@:2}" --state-dir "$(ct_gateway_dir)/peer-jobs"
    ;;
  peer-route)
    peer_dispatch route "${@:2}"
    ;;
esac

case "$CMD" in
  process-probe|lane-wrapper-check|cache-wrapper-check|dev-link-measure|feature-cycle|embedding-parity|fmt-check|check|check-tests|ci-tests|ci-clippy|ci-cli-tests|release-inputs|release-bundle|release-finalize|release-arm64-compile|release-preflight|release-rust|test|test-one|nextest|verify|worktree-check|build|qa-prep|bootstrap|cli|cli-docs|clean|test:all)
    enter_cargo_gateway "$CMD ${*:2}"
    ;;
  frontend-build)
    enter_build_gateway "$CMD"
    ;;
esac

case "$CMD" in
  help)
    cat <<'EOF'
Domios build commands

  dev                         Start the visible shared dev instance
  dev-prebuilt                Restart a validated dev build without Cargo
  dev-env                     Create or refresh the ignored local build .env from the vault once
  dev-back                    DEV server + native window; the frontend comes from dev-front
  dev-front                   DEV frontend on Vite with hot reload (pair with dev-back)
  dev-server                  Rebuild/reload only the selected DEV server; keep the window
  retain-pdb <sha> [pdb] [key]  Keep a linked dev build's symbol file (last 3)
  check                       Fast Rust compile check
  check-tests                 Compile app and CLI test profiles
  embedding-parity [--baseline FILE] [--old-binary FILE] [--output FILE]  CPU MiniLM golden parity and RSS/latency
  lane-check <package>... [--tests] [--timings]  Type-check touched packages in one Cargo check with four jobs
  fmt [--base REF]          Format changed Rust and apply changed-file Biome safe fixes
  lane-wrapper-check          Verify cache forwarding and metadata isolation with tiny fixtures
  cache-wrapper-check         Verify compiler-cache relocation and tracked environment with real sccache
  dev-link-measure [app|cli-release|cli-dev]  Record Cargo timings without installing or launching DEV
  feature-cycle before|after <selector>  Measure equivalent dev/check/test core units in a temporary worktree
  check-queue [sha] [--lane|--all] Warm touched lane or full batch Clippy check in the per-machine check worktree
  check-queue <sha> --peer <name|auto> [--lane] [--os OS]  Same check in a mesh peer's gateway; result returns as one message
  nextest <scope> [filterset] --peer <name|auto> [--sha SHA]  Same nextest run in a mesh peer's gateway
  test-one <scope> <name> --peer <name|auto> [--sha SHA]  Same focused test in a peer gateway; any peer verb takes --prereq DIFF (local, never-pushed commit)
  check-linux [sha] [--force]  Once per batch: cargo check --workspace --all-targets on a Linux peer
  check-workspace [sha]       Whole-workspace cargo check of a SHA in this machine's check worktree
  peer-setup <peer> [--sha R] [--install-toolchain]  Clone/refresh the peer fetch-only checkout, check (or user-install) its toolchain, open its status tab
  check-series --spec F --peer P  Serial scratch-commit check-queue timings (seed, floor, edit per area) on a peer
  peer-route [--os OS]        Show which peer --peer auto would pick
  peer-status                 One PEER_STATUS line: this machine's gateway load (read by --peer auto)
  peer-toolchain [--install]   Check rustup/rustc pin, native toolchain, node, nextest, sccache; --install adds user-scope rustup + nextest
  runner-pause MACHINE        Park MACHINE-* GitHub runners (labels -> paused; unique labels lent to a same-OS runner)
  runner-resume MACHINE       Restore MACHINE-* runner labels and return lent labels
  runner-status               List runners with labels and paused machines
  frontend                    Production frontend typecheck + bundle
  release-inputs [--for-bundle] Build target-aware inputs; defer native compilation for bundling
  release-bundle [--fresh-target] [tauri args...]  Package in one session; optionally use the fresh release target policy
  release-arm64-compile TAG SHA ARCHIVE  Build unsigned hosted bundles and freeze handoff in one session
  release-finalize [stage args...]  Verify/package final bytes while the stable gateway path exists
  ci-tests full|platform|embeddings [compile]  Compile and run ordinary, OS-specific, or embeddings Rust tests; compile stops after building
  ci-clippy                   Run the clippy repo gate through the shared compile cache
  ci-cli-tests                Run every CLI test target (contracts included) in the fast tier
  frontend-build              Run the release bundle's frontend step from its stable build path, without Rust
  worktree-check              Isolated lane-safe Rust compile check
  test-one <input-macos|input-windows|billing|account|analytics|app|shell|root|api|socket|commands|sentinel-host|frontend-bundle|plan|pty|pty-fork|util|browser|types|wire|sentinel|decision|provider-signals|storage|mem|mem-store|paths|mesh|plugin|cli|agent-detect|notification-core|repowatch|usage|codeterm-*> NAME
                              Run one focused Rust test
  process-probe --case server-only|arbitration|pair-journey --instance split-oracle  Build and prove isolated process ownership
  nextest <scope> [filterset]  Compile then run Rust tests through nextest
  test-related <file>...     Print and run Rust module/crate and TS related selections
  nextest-archive [sha] [--measure selector...] Build/reuse one archive and optionally time selectors
  release-rust [runner-id...]  Run every release-tier Rust runner of tests/catalogue.json (or the named ones)
  test                        Run the application Rust unit suite
  verify                      Run app/CLI tests, TypeScript check, and source gates
  qa-prep                     Prepare a worktree for QA
  chat-scroll-trace [--webkit] [--fix 0|1] [--strokes N] [--scenario touch|grow|chaos] [--seed N] [--actions N] [--out DIR] [--cdp WS]
                              Scripted iPhone touch scroll over the mounted Chat window (grow: rows expand and collapse
                              while pinned, scrolled up and scrolling; chaos: a seeded mix of strokes, flings, the jump
                              button, arriving rows and growth anywhere); fails on any visual jump
  install-local               Build and install locally (cheap fmt/size/clean gates first)
  script-guards               Run the script catalogue gate and uncalled fixture guards
  qa-lock-storm [--seconds N] --out FILE  Measure DEV routes and lock holds (default 120s; --notify-tab supported)
  qa-batch <label> '<cmd>'...  Run check first, then the commands, one QA receipt
  lane-recover keep|merge ...  Park a wrong-base lane on keep/<lane>, then merge it back

Long operations (dev, dev-prebuilt, check, check-tests, test-one, test, verify, build,
install-local, release-preflight, release-rust) send one success or failure receipt
inside Domios. --notify-tab <tab> or BUILD_NOTIFY_TAB overrides the recipient.
  build                       Build production artifacts
  cache-status                Show configured/server cache locations and endpoint health
  cache-verify                Verify shared S3 storage; in CI with the store down, start a job-local cache
  cache-release               Stop and remove a job-local cache started by cache-verify
  cache-stats                 Show gateway cache deltas by job kind and live server counters
  gateway-status              Show the machine build owner
  gateway-relocate            Windows: move idle gateway state out of the app install folder
  build-history [--last N] [--verb V] [--json]  Build durations, cache hits and rebuild reasons
  gc [--dry-run]              Reclaim expired build caches and sweep checkout targets

Gateway-backed commands fail fast when the slot is busy. Add --wait-build only
when blocking this terminal is intentional.
EOF
    ;;
  env)
    # Already loaded above. Use: source build.sh env
    ;;
  dev-env)
    CODETERM_ENV_VAULT_CLI="${DEV_NOTIFY_PARENT_CLI:-codeterm}" node scripts/local-build-env.mjs create
    ;;
  gc)
    if [ "$#" -gt 2 ] || { [ "$#" -eq 2 ] && [ "$2" != "--dry-run" ]; }; then
      echo "Usage: ./build.sh gc [--dry-run]" >&2
      exit 2
    fi
    enter_build_gateway "gc"
    ct_cache_retention gc "${2:-}"
    ;;
  fmt)
    ensure_deps
    node scripts/format-changed.mjs "${@:2}"
    ;;
  fmt-check)
    bash scripts/check-rustfmt.sh
    ;;
  cache-status)
    ct_sccache_status
    ;;

  cache-verify)
    ct_sccache_verify
    ;;
  cache-release)
    ct_sccache_release_job_local
    ;;
  cache-stats)
    node scripts/cache-stats.mjs --dir "$(ct_gateway_dir)"
    if ct_sccache_server_live; then
      echo "Live sccache process counters (reset when the server process restarts):"
      ct_sccache_stats || echo "[cache-policy] live sccache counters unavailable"
    else
      echo "Live sccache process counters: unavailable (no server is running)"
    fi
    ;;
  gateway-status)
    ct_gateway_status
    ;;
  gateway-relocate)
    ct_gateway_relocate strict
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
        complete_build_gateway_before_handoff
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
        bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "${CT_GATEWAY_OWNER_PID:-$$}" "$CT_GATEWAY_PROCESS_START" "qa-prep" --probe-only
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
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    check_log="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.cargo-check.log"
    cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | tee "$check_log"
    check_status="${PIPESTATUS[0]}"
    check_warning_summary < "$check_log"
    exit "$check_status"
    ;;
  check-queue)
    check_queue_run "${@:2}"
    ;;
  check-workspace)
    check_workspace_run "${@:2}"
    ;;
  peer-status)
    ct_gateway_peer_status
    ;;
  peer-toolchain)
    peer_toolchain "${@:2}"
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
      errors) check_queue_errors "${3:?errors requires a max}" "${4:-0}" ;;
      warnings) check_queue_warnings "${3:?warnings requires a max}" "${4:-}" ;;
      summary) check_warning_summary ;;
      *) echo "Unknown check-queue probe: ${2:-}" >&2; exit 2 ;;
    esac
    ;;
  lane-check)
    if [ "$CODETERM_BUILD_GATEWAY_MODE" = queue ]; then
      enter_cargo_gateway "$CMD ${*:2}"
    fi
    lane_check "${@:2}"
    ;;
  lane-wrapper-check)
    node --test scripts/tests/lane-check.test.mjs
    ;;
  cache-wrapper-check)
    CODETERM_CACHE_WRAPPER_TEST=1 node --test scripts/tests/cache-rustc-wrapper.test.mjs scripts/tests/cache-wrapper-identity.test.mjs
    ;;
  dev-link-measure)
    if [ "${CODETERM_LINK_TIMING:-0}" = 1 ]; then
      case "$OSTYPE" in
        msys*|cygwin*|win32)
          export LINK="${LINK:+$LINK }/TIME"
          export MSYS2_ENV_CONV_EXCL="${MSYS2_ENV_CONV_EXCL:+$MSYS2_ENV_CONV_EXCL;}LINK"
          export RUSTC_LOG="${RUSTC_LOG:+$RUSTC_LOG,}rustc_codegen_ssa::back::link=info"
          ;;
      esac
    fi
    case "${2:-app}" in
      cli-release|cli-dev)
        measure_cli_args=(--manifest-path cli/Cargo.toml --timings)
        [ "$2" != cli-release ] || measure_cli_args+=(--release)
        cargo build "${measure_cli_args[@]}"
        exit $?
        ;;
      app) ;;
      *) echo 'Usage: ./build.sh dev-link-measure [app|cli-release|cli-dev]' >&2; exit 2 ;;
    esac
    export CODETERM_DEV=1
    export CODETERM_WEB_DIST="$(pwd -P)/dist"
    configure_dev_cargo
    prepare_dev_frontend_dist
    prepare_bundle_stubs
    measure_args=(--manifest-path src-tauri/Cargo.toml -p codeterm --bin domios-app
      --profile "$CT_DEV_CARGO_PROFILE" --no-default-features --timings)
    TAURI_CONFIG="$(cat "$(dev_conf_path)")" cargo build "${measure_args[@]}"
    ;;
  embedding-parity)
    CARGO_BUILD_JOBS=4 cargo build --release --manifest-path src-tauri/Cargo.toml \
      -p codeterm-mem --example embedding_oracle
    oracle_dir="${CARGO_TARGET_DIR:-src-tauri/target}"
    if [ -n "${CARGO_BUILD_TARGET:-}" ]; then oracle_dir="$oracle_dir/$CARGO_BUILD_TARGET"; fi
    oracle_ext=""
    case "$OSTYPE" in msys*|cygwin*|win32) oracle_ext=".exe" ;; esac
    python scripts/embedding-parity.py --new-binary "$oracle_dir/release/examples/embedding_oracle$oracle_ext" "${@:2}"
    ;;
  feature-cycle)
    source scripts/feature-cycle.sh
    feature_cycle "${@:2}"
    ;;
  check-tests)
    prepare_exceptional_worktree_target
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    cargo test --manifest-path src-tauri/Cargo.toml --workspace --exclude codeterm-cli --all-targets --no-default-features --no-run
    cargo test --manifest-path cli/Cargo.toml --all-targets --no-run
    ;;
  frontend)
    enter_build_gateway "$CMD ${*:2}"
    ensure_deps
    pnpm build
    ;;
  ci-clippy)
    prepare_bundle_stubs
    bash scripts/clippy-gate.sh
    ;;
  ci-cli-tests)
    prepare_bundle_stubs
    cli_fast_args=$(node scripts/ci-cli-targets.mjs) || return $?
    [ -n "$cli_fast_args" ] || return 1
    cli_fast_targets=()
    while IFS= read -r cli_fast_arg; do cli_fast_targets+=("$cli_fast_arg"); done <<< "$cli_fast_args"
    scripts/test-capped.sh --no-default-features --manifest-path cli/Cargo.toml "${cli_fast_targets[@]}" --locked
    ;;
  ci-tests)
    case "${2:-}" in full|platform|embeddings) ;; *) echo "Usage: ./build.sh ci-tests full|platform|embeddings [compile]" >&2; exit 2 ;; esac
    case "${3:-}" in ''|compile) ;; *) echo "Usage: ./build.sh ci-tests full|platform|embeddings [compile]" >&2; exit 2 ;; esac
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    node scripts/collect-rust-warnings.mjs run --log "${RUST_WARNING_LOG:-ci-rust-warnings.log}" -- node scripts/ci-rust-tests.mjs "$2" ${3:+--compile-only}
    ;;
  frontend-build)
    ensure_deps
    pnpm build
    ;;
  release-inputs)
    [ "$#" -le 2 ] && { [ -z "${2:-}" ] || [ "$2" = --for-bundle ]; } || {
      echo "Usage: ./build.sh release-inputs [--for-bundle]" >&2; exit 2;
    }
    ensure_deps
    if [ "${2:-}" = --for-bundle ]; then
      prepare_release_bundle_inputs bundle
    else
      prepare_release_bundle_inputs
    fi
    ;;
  # One session keeps both compiles on the gateway's stable path: a second path is a second
  # set of Cargo fingerprints and sccache keys, so tauri build would recompile the app.
  release-bundle)
    bundle_args=("${@:2}")
    if [ "${2:-}" = "--fresh-target" ]; then
      require_release_source
      prepare_release_target
      bundle_args=("${@:3}")
    fi
    ensure_deps
    prepare_release_bundle_inputs bundle
    bundle_target_args=()
    [ -z "${CARGO_BUILD_TARGET:-}" ] || bundle_target_args=(--target "$CARGO_BUILD_TARGET")
    if [ "${CODETERM_RELEASE_COMMON_STAGE:-0}" = 1 ]; then
      [ "${#bundle_args[@]}" -eq 0 ] || { echo "Shared-stage release-bundle takes no extra Tauri flags" >&2; exit 2; }
      bundle_common_stage
    else
      pnpm tauri build "${bundle_target_args[@]}" ${CODETERM_RELEASE_FEATURES:+--features "$CODETERM_RELEASE_FEATURES"} "${bundle_args[@]}"
    fi
    ;;
  release-finalize)
    node scripts/release-stage.mjs finalize "${@:2}"
    ;;
  release-arm64-compile)
    case "${CARGO_BUILD_TARGET:-}" in
      aarch64-unknown-linux-gnu|aarch64-pc-windows-msvc) ;;
      *) echo "Hosted compilation requires an explicit arm64 target" >&2; exit 2 ;;
    esac
    [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ] || { echo "Hosted compilation must not receive signing credentials" >&2; exit 2; }
    export CODETERM_RELEASE_COMMON_STAGE=1 CODETERM_RELEASE_UNSIGNED=1
    ensure_deps
    prepare_release_bundle_inputs bundle
    bundle_common_stage
    node scripts/release-arm64-handoff.mjs pack "$CARGO_BUILD_TARGET" "${2:?tag required}" "${3:?source SHA required}" "${4:?archive required}"
    ;;
  release-preflight)
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ensure_deps
    bash scripts/check-rustfmt.sh
    bash scripts/check-file-sizes.sh --fail
    node scripts/check-provider-boundary.mjs
    bash scripts/check-tauri-reach.sh
    bash scripts/check-crate-layering.sh
    bash scripts/check-empty-array-expansion.sh
    # esbuild cannot relativize a linked workspace package across the gateway's subst drive,
    # so the bundle diff runs from the real worktree where the committed bundles were built.
    (cd "${CT_REAL_WORKTREE:-.}" && node scripts/check-plugin-bundles.mjs)
    node scripts/sync-plugin-versions.mjs --check
    cargo check --manifest-path src-tauri/Cargo.toml
    cargo check --manifest-path cli/Cargo.toml
    node scripts/check-release-locks.mjs
    # The script, gateway and release-signing fixture guards run here, not in a PR workflow.
    # Under the gateway's subst drive the catalogue reads every Cargo.toml as an unmatched test; run from the real worktree.
    (cd "${CT_REAL_WORKTREE:-.}" && bash ./build.sh script-guards)
    bash scripts/clippy-gate.sh
    run_release_rust_tests
    node node_modules/typescript/bin/tsc --noEmit
    node node_modules/knip/bin/knip.js --workspace @codeterm/chat-core --exports
    node scripts/check-api-effect-dependencies.mjs
    node scripts/run-ts-unit-tests.mjs
    node node_modules/vitest/vitest.mjs run
    node scripts/run-plugin-tests.mjs
    node --test scripts/tests/updater-manifest.test.mjs
    node --test scripts/tests/bundled-cli-name.test.mjs
    node --test scripts/tests/local-install-cache.test.mjs
    node node_modules/@biomejs/biome/bin/biome check .
    ;;
  release-rust)
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    run_release_rust_tests "${@:2}"
    ;;
  test)
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    run_workspace_lib_tests
    ;;
  process-probe)
    [ -z "$(git status --porcelain)" ] || { echo 'ERROR: process-probe requires the exact committed clean source' >&2; exit 2; }
    node scripts/process-separation-probe.mjs --validate "${@:2}"
    process_probe_case=""
    process_probe_args=("${@:2}")
    for ((process_probe_arg=0; process_probe_arg<${#process_probe_args[@]}; process_probe_arg++)); do
      if [ "${process_probe_args[$process_probe_arg]}" = --case ]; then
        process_probe_value=$((process_probe_arg + 1))
        process_probe_case="${process_probe_args[$process_probe_value]:-}"
        break
      fi
    done
    process_probe_dir="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID-process-probe"
    mkdir -p "$process_probe_dir"
    process_probe_target="$(ct_host_triple)"
    cargo build --manifest-path cli/Cargo.toml --bin codeterm --target "$process_probe_target" \
      --message-format=json > "$process_probe_dir/cli.jsonl"
    CODETERM_SERVER_CLI_SHA256="$(node scripts/server-artifact.mjs stage-resources \
      "$process_probe_dir/cli.jsonl" "$(pwd -P)" "$process_probe_dir/resources")"
    export CODETERM_SERVER_CLI_SHA256
    cargo build --manifest-path src-tauri/Cargo.toml -p domios-server --bin domios-server \
      --no-default-features --features domios-server/embeddings,domios-server/mem-postgres --target "$process_probe_target" \
      --message-format=json > "$process_probe_dir/cargo.jsonl"
    cargo tree --manifest-path src-tauri/Cargo.toml -p domios-server --target "$process_probe_target" \
      --no-default-features --features domios-server/embeddings,domios-server/mem-postgres --edges normal,build --prefix none \
      --format '{p}' > "$process_probe_dir/closure.txt"
    node scripts/server-artifact.mjs "$process_probe_dir/cargo.jsonl" "$process_probe_dir/closure.txt" \
      "$(git rev-parse HEAD)" "$process_probe_target" "$process_probe_dir/resources" "$process_probe_dir/artifact.json"
    client_receipt_args=()
    if [ "$process_probe_case" = pair-journey ]; then
      IFS=$'\t' read -r process_probe_client_package process_probe_client_binary \
        < <(node scripts/process-separation-probe.mjs --client-build-target)
      cargo build --manifest-path src-tauri/Cargo.toml -p "$process_probe_client_package" --bin "$process_probe_client_binary" \
        --target "$process_probe_target" --message-format=json > "$process_probe_dir/client.jsonl"
      node scripts/process-separation-probe.mjs --write-client-receipt "$process_probe_dir/client.jsonl" \
        "$process_probe_dir/client-artifact.json" "$(git rev-parse HEAD)" "$process_probe_target"
      client_receipt_args=(--client-receipt "$process_probe_dir/client-artifact.json")
    fi
    node scripts/process-separation-probe.mjs "${@:2}" --artifact-receipt "$process_probe_dir/artifact.json" "${client_receipt_args[@]}"
    ;;
  test-one)
    test_scope="${2:?test-one requires domios-server, billing, account, analytics, app, shell, root, api, socket, commands, sentinel-host, frontend-bundle, plan, pty, pty-fork, util, browser, types, wire, sentinel, decision, provider-signals, storage, mem, mem-store, paths, mesh, plugin, plugins, agent-detect, notification-core, repowatch, usage, cli, or a codeterm-* package}"
    test_filter="${3:?test-one requires a test name or filter}"
    case "$test_scope" in
      cli|codeterm-cli)
        SLOW_UNIT_TESTS_MEASUREMENT="${SLOW_UNIT_TESTS_MEASUREMENT:-off}" run_nextest_scope cli "package(=codeterm-cli) & test(~$test_filter)"
        exit $?
        ;;
      frontend-bundle|codeterm-frontend-bundle|input-macos|input-windows|pty-fork) ;;
      *)
        if [ "$test_filter" != build_identity_required ]; then
          # Same workspace feature resolution as every other test run, so the warm build is reused.
          [ -z "${RUST_TEST_THREADS:-}" ] || export NEXTEST_TEST_THREADS="${NEXTEST_TEST_THREADS:-$RUST_TEST_THREADS}"
          test_package="$(rust_test_package "$test_scope")" || exit $?
          SLOW_UNIT_TESTS_MEASUREMENT="${SLOW_UNIT_TESTS_MEASUREMENT:-off}" run_nextest_scope workspace "package(=$test_package) & test(~$test_filter)"
          exit $?
        fi
        ;;
    esac
    case "$test_scope" in
      domios-server)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p domios-server --lib "$test_filter"
        ;;
      app)
        if [ "$test_filter" = "build_identity_required" ]; then
          scripts/test-capped.sh --exec node --test scripts/tests/build_identity_required.test.mjs
          exit $?
        fi
        prepare_dev_frontend_dist "$(pwd -P)"
        prepare_bundle_stubs
        prepare_exceptional_worktree_target
        if find_app_test_archive; then
          run_archived_app_test "$test_filter"
        else
          configure_app_test_cargo test-one
          scripts/test-capped.sh "${CT_APP_TEST_ARGS[@]}" "$test_filter"
        fi
        ;;
      shell)
        prepare_dev_frontend_dist "$(pwd -P)"
        prepare_bundle_stubs
        prepare_exceptional_worktree_target
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm --lib "$test_filter"
        ;;
      mem-store|codeterm-mem-store) scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-mem-store --lib "$test_filter" ;;
      paths|codeterm-paths) scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-paths --lib "$test_filter" ;;
      input-macos|input-windows)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p "codeterm-$test_scope" --lib "$test_filter"
        ;;
      root)
        prepare_dev_frontend_dist "$(pwd -P)"
        prepare_bundle_stubs
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm --lib "$test_filter"
        ;;
      api|socket|commands|sentinel-host|codeterm-api|codeterm-socket|codeterm-commands|codeterm-sentinel-host)
        prepare_dev_frontend_dist "$(pwd -P)"
        prepare_bundle_stubs
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p "codeterm-${test_scope#codeterm-}" --lib "$test_filter"
        ;;
      frontend-bundle|codeterm-frontend-bundle)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-frontend-bundle --lib "$test_filter"
        ;;
      agent-detect|codeterm-agent-detect)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-agent-detect --lib "$test_filter"
        ;;
      billing) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-billing/Cargo.toml -p codeterm-billing --lib "$test_filter" ;;
      usage)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-usage/Cargo.toml -p codeterm-usage --lib "$test_filter"
        ;;
      account) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-account/Cargo.toml -p codeterm-account --lib "$test_filter" ;;
      notification-core|codeterm-notification-core)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-notification-core --lib "$test_filter"
        ;;
      analytics) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-analytics/Cargo.toml -p codeterm-analytics --lib "$test_filter" ;;
      repowatch|codeterm-repowatch)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-repowatch --lib "$test_filter"
        ;;
      plan) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-plan/Cargo.toml -p codeterm-plan --lib "$test_filter" ;;
      pty) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-pty/Cargo.toml -p codeterm-pty "$test_filter" ;;
      # pty-fork is excluded from the workspace (its own Cargo.lock) but owns
      # the conpty test-isolation boundary (win/psuedocon.rs). Kept as its own
      # scope rather than chained onto `pty` so a pre-existing standalone-build
      # issue in that vendored, previously-untested crate can't newly break
      # the working codeterm-pty test path.
      pty-fork) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/pty-fork/Cargo.toml "$test_filter" ;;
      util|codeterm-util)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-util --lib "$test_filter"
        ;;
      browser|codeterm-browser)
        scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p codeterm-browser --lib "$test_filter"
        ;;
      types) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-types/Cargo.toml -p codeterm-types --all-targets "$test_filter" ;;
      wire) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-wire/Cargo.toml -p codeterm-wire --all-targets "$test_filter" ;;
      sentinel) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-sentinel/Cargo.toml -p codeterm-sentinel --all-targets "$test_filter" ;;
      decision) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-decision/Cargo.toml -p codeterm-decision --all-targets "$test_filter" ;;
      provider-signals) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-provider-signals/Cargo.toml -p codeterm-provider-signals --all-targets "$test_filter" ;;
      storage) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-storage/Cargo.toml -p codeterm-storage --lib "$test_filter" ;;
      mem) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-mem/Cargo.toml -p codeterm-mem --lib "$test_filter" ;;
      mesh) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-mesh/Cargo.toml -p codeterm-mesh --lib "$test_filter" ;;
      blueprint-types|codeterm-blueprint-types) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-blueprint-types/Cargo.toml -p codeterm-blueprint-types --lib "$test_filter" ;;
      local-recording|codeterm-local-recording) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-local-recording/Cargo.toml -p codeterm-local-recording --lib "$test_filter" ;;
      plugin) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-plugin/Cargo.toml -p codeterm-plugin --lib "$test_filter" ;;
      # Plural crate (channel/install/update), distinct from the singular confinement crate above.
      plugins) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/crates/codeterm-plugins/Cargo.toml -p codeterm-plugins --lib "$test_filter" ;;
      cli) scripts/test-capped.sh --no-default-features --manifest-path cli/Cargo.toml --all-targets "$test_filter" ;;
      codeterm-*) scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml -p "$test_scope" --lib "$test_filter" ;;
      *) echo "Unknown test scope: $test_scope (expected billing, account, analytics, app, shell, root, api, socket, commands, sentinel-host, frontend-bundle, plan, pty, pty-fork, util, browser, types, wire, sentinel, decision, provider-signals, storage, mem, mem-store, paths, mesh, plugin, plugins, agent-detect, notification-core, repowatch, usage, cli, or a codeterm-* package)" >&2; exit 2 ;;
    esac
    ;;
  nextest)
    run_nextest_scope "${@:2}"
    ;;
  nextest-archive)
    build_app_test_archive "${@:2}"
    ;;
  verify)
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ensure_deps
    cargo check --manifest-path src-tauri/Cargo.toml
    run_workspace_lib_tests
    TEST_CAP_SECS="${TEST_CAP_SECS:-3600}" scripts/test-capped.sh --no-default-features --manifest-path cli/Cargo.toml
    node node_modules/typescript/bin/tsc --noEmit
    bash scripts/check-file-sizes.sh --fail
    bash scripts/check-service-core-boundary.sh
    node scripts/check-provider-boundary.mjs
    bash scripts/check-tauri-reach.sh
    bash scripts/check-empty-array-expansion.sh
    ;;
  worktree-check)
    prepare_exceptional_worktree_target
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    cargo check --manifest-path src-tauri/Cargo.toml --all-targets "${@:2}"
    ;;
  build)
    build_notarize=1
    for arg in "${@:2}"; do
      case "$arg" in
        --no-notarize) build_notarize=0 ;;
        *) echo "Usage: ./build.sh build [--no-notarize]" >&2; exit 2 ;;
      esac
    done
    node scripts/check-release-locks.mjs
    require_release_source
    build_platform="$(package_platform "$(ct_host_triple)")"
    # The macOS app bundle never carries updater artifacts, so only the other bundlers need the updater key.
    if [ "$build_platform" = macos ]; then
      prepare_local_macos_signing "$build_notarize"
    else
      ct_load_release_signing_key
    fi
    prepare_release_target
    ensure_deps
    # Local packages stage the client/server pair exactly as release CI does.
    export CODETERM_RELEASE_COMMON_STAGE=1
    prepare_release_bundle_inputs bundle
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    bundle_common_stage ct_with_firebase_build_config
    ct_check_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    bundle_root="$CARGO_TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release/bundle"
    if [ "$build_platform" = macos ] && [ "$build_notarize" = 1 ]; then
      notarize_local_macos_package "$bundle_root"
    fi
    packages="$(find "$bundle_root" -maxdepth 2 -type f \( -name '*.dmg' -o -name '*-setup.exe' -o -name '*.deb' -o -name '*.AppImage' \))"
    [ -n "$packages" ] || { echo "ERROR: no local package under $bundle_root" >&2; exit 1; }
    while IFS= read -r package; do
      command -v cygpath >/dev/null 2>&1 && package="$(cygpath -w "$package")"
      echo "Local package ready: $package"
    done <<< "$packages"
    echo "Install with: codeterm update install --from-file <package>"
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
    enter_cargo_gateway "$CMD ${*:2}"
    bash scripts/check-rustfmt.sh
    bash scripts/check-file-sizes.sh --fail
    require_release_source
    node scripts/check-release-locks.mjs
    ct_load_release_signing_key
    prepare_local_install_target
    ensure_deps
    # The installed client/server pair packages exactly as the release does, from the shared stage.
    export CODETERM_RELEASE_COMMON_STAGE=1
    prepare_release_bundle_inputs bundle
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    rm -rf "$CARGO_TARGET_DIR/release/bundle/nsis"
    bundle_common_stage ct_with_firebase_build_config
    ct_check_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    VERSION="$(node -p "require('./package.json').version")"
    INSTALLER="$CARGO_TARGET_DIR/release/bundle/nsis/Domios_${VERSION}_x64-setup.exe"
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
      echo "Post-install continuation: disabled (no calling Domios Tab)"
      INSTALL_ARGS+=(--no-continue)
    fi
    complete_build_gateway_before_handoff
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
    sg_run node scripts/test-catalogue.mjs validate
    sg_run node scripts/rename-guard.mjs --scope all --report
    sg_run bash scripts/check-crate-layering.sh
    sg_run node --test scripts/tests/crate-layering.test.mjs
    sg_run node --test scripts/check-queue-plan.test.mjs scripts/nextest-archive.test.mjs scripts/check-slow-unit-tests.test.mjs
    sg_run bash scripts/check-build-gateway-owner-reap.sh
    sg_run bash scripts/check-build-check-queue.sh
    sg_run bash scripts/check-build-script-head.sh
    if command -v python3 >/dev/null 2>&1; then
      sg_run python3 scripts/check-lf-working-tree.py
    else
      sg_run python scripts/check-lf-working-tree.py
    fi
    sg_run bash scripts/check-shared-sccache.sh
    for sg in check-build-isolated-dev-policy.sh check-dev-resource-preflight.sh check-health-identity.sh check-local-install-continuation.sh       check-no-raw-graph-sql.sh check-visual-golden-edits.sh test-clippy-gate.sh test-descendant-cleanup.sh test-merge-stack-audit.sh test-timeout-seam.sh; do
      sg_run bash "scripts/$sg"
    done
    sg_run bash scripts/tests/firebase-build-config.test.sh
    if [ "$(uname -s)" = Linux ]; then
      # POSIX process and file-descriptor fixtures; policy also covers release signing and source-clean.
      for sg in check-build-gateway-serialization.sh check-build-gateway-policy.sh check-build-gateway-mutex-cleanup.sh; do
        sg_run bash "scripts/$sg"
      done
      sg_run bash scripts/check-build-dev-port-overrides.sh
      sg_run bash scripts/check-cache-provenance.sh
      sg_run node scripts/test-catalogue.mjs run --entry node-script-tests-cargo
      sg_run node scripts/test-catalogue.mjs run --entry node-script-tests-serial
    fi
    exit "$sg_failed"
    ;;
  qa-lock-storm)
    storm_seconds=120
    storm_out=""
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --seconds|--out)
          [ "$#" -ge 2 ] || { echo "ERROR: $1 requires a value" >&2; exit 2; }
          case "$1" in --seconds) storm_seconds="$2" ;; --out) storm_out="$2" ;; esac
          shift 2 ;;
        *) echo "ERROR: usage: qa-lock-storm [--seconds N] --out FILE" >&2; exit 2 ;;
      esac
    done
    [[ "$storm_seconds" =~ ^[1-9][0-9]*$ ]] && [ "$storm_seconds" -le 3600 ] && [ -n "$storm_out" ] || {
      echo "ERROR: --seconds must be 1..3600 and --out FILE is required" >&2; exit 2;
    }
    source evals/_lib/lib.sh
    dev_resolve
    storm_tabs="$(dev_get /api/tabs)"
    storm_tab="$(printf '%s' "$storm_tabs" | python3 -c 'import json,sys; tabs=json.load(sys.stdin); print(tabs[0]["id"] if tabs else "")')"
    [ -n "$storm_tab" ] || { echo "ERROR: DEV needs an existing tab" >&2; exit 1; }
    storm_log="$(dev_data_dir)/logs/domios.log"
    storm_bash="$BASH"
    if is_windows; then
      storm_bash="$(cygpath -m "$storm_bash")"
      storm_log="$(cygpath -m "$storm_log")"
      storm_out="$(cygpath -m "$storm_out")"
    fi
    dev_run python3 - "$storm_seconds" "$storm_out" "$storm_tab" "$storm_log" "$storm_bash" <<'LOCK_STORM_PY'
import collections
import concurrent.futures
import datetime
import json
import math
import pathlib
import re
import subprocess
import sys
import threading
import time


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def lock_histogram(lines, start, end):
    locks, sites = collections.Counter(), collections.Counter()
    unmatched = 0
    for line in lines:
        if "[LOCK-HOLD]" not in line:
            continue
        stamp = re.search(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", line)
        if not stamp:
            continue
        moment = datetime.datetime.fromisoformat(stamp[0].replace("Z", "+00:00")).timestamp()
        # The logger has second precision: include both boundary seconds.
        if not math.floor(start) <= moment <= math.floor(end):
            continue
        match = re.search(r"\[LOCK-HOLD\] (.+?)\.\w+\(\) held \d+ms \(waited \d+ms\) at (.+:\d+)", line)
        if match:
            locks[match[1]] += 1
            sites[f"{match[1]} at {match[2]}"] += 1
        else:
            unmatched += 1
    return {"per_lock": dict(locks), "per_site": dict(sites), "unmatched": unmatched}


WORKER = r'''
while IFS= read -r route; do
  route="${route%$'\r'}"
  curl -sS --max-time 30 -o /dev/null -w '%{time_total} %{http_code}\n' \
    -H "Authorization: Bearer $CODETERM_API_TOKEN" "$CODETERM_API_URL$route" || true
done
'''


def measure(route, deadline):
    values, statuses = [], collections.Counter()
    with subprocess.Popen([sys.argv[5], "-c", WORKER], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, text=True) as worker:
        next_tick = time.monotonic()
        while time.monotonic() < deadline:
            worker.stdin.write(route + "\n")
            worker.stdin.flush()
            response = worker.stdout.readline().split()
            if len(response) != 2:
                raise RuntimeError(f"DEV helper exited for {route}")
            values.append(float(response[0]) * 1000)
            statuses[response[1] if response[1] != "000" else "transport_error"] += 1
            next_tick += 0.250
            # Cadence generates load; it never establishes readiness or success.
            if next_tick < time.monotonic():
                next_tick = time.monotonic()
            threading.Event().wait(max(0, min(next_tick, deadline) - time.monotonic()))
        worker.stdin.close()
        worker.wait(timeout=35)
    return {"samples": len(values), "p50_ms": percentile(values, .50),
            "p95_ms": percentile(values, .95), "max_ms": max(values, default=None),
            "status_counts": dict(statuses)}


def main():
    seconds, out, tab, log = int(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], pathlib.Path(sys.argv[4])
    routes = ["/api/mux/state", "/api/tabs", f"/api/tabs/{tab}/chat"]
    start, deadline = time.time(), time.monotonic() + seconds
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        futures = {route: pool.submit(measure, route, deadline) for route in routes}
        measurements = {route: future.result() for route, future in futures.items()}
    end = time.time()
    with log.open(encoding="utf-8", errors="replace") as stream:
        holds = lock_histogram(stream, start, end)
    result = {"start_epoch": start, "end_epoch": end, "requested_seconds": seconds,
              "cadence_ms": 250, "latency_scope": "direct HTTP time_total; DEV identity checked once before sampling",
              "log_window_precision": "inclusive boundary seconds", "log_path": str(log),
              "routes": measurements, "lock_holds": holds}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print("Route                                      n    p50 ms    p95 ms    max ms  statuses")
    for route, stats in measurements.items():
        print(f"{route:42} {stats['samples']:4} {stats['p50_ms'] or 0:9.1f} {stats['p95_ms'] or 0:9.1f} {stats['max_ms'] or 0:9.1f}  {stats['status_counts']}")
    for group in ("per_lock", "per_site"):
        print(f"LOCK-HOLD {group}:")
        for name, count in sorted(holds[group].items(), key=lambda item: (-item[1], item[0])):
            print(f"  {count:5}  {name}")
        if not holds[group]:
            print("      0  (none)")
    print(f"Unmatched LOCK-HOLD lines: {holds['unmatched']}; JSON: {out}")
    print("Latency measures direct HTTP requests. Log window includes both boundary seconds.")


if __name__ == "__main__":
    main()
LOCK_STORM_PY
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
  chat-scroll-trace)
    cst_webkit=0
    cst_fix=1
    cst_strokes=14
    cst_out=""
    cst_cdp="${CHAT_SCROLL_TRACE_CDP:-}"
    cst_scenario="touch"
    cst_seed=1
    cst_actions=80
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --webkit) cst_webkit=1 ;;
        --scenario) cst_scenario="${2:-}"; shift ;;
        --seed) cst_seed="${2:-}"; shift ;;
        --actions) cst_actions="${2:-}"; shift ;;
        --fix) cst_fix="${2:-}"; shift ;;
        --strokes) cst_strokes="${2:-}"; shift ;;
        --out) cst_out="${2:-}"; shift ;;
        --cdp) cst_cdp="${2:-}"; shift ;;
        *) echo "ERROR: usage: chat-scroll-trace [--webkit] [--fix 0|1] [--strokes N] [--scenario touch|grow|chaos] [--seed N] [--actions N] [--out DIR] [--cdp WS]" >&2; exit 2 ;;
      esac
      shift
    done
    case "$cst_fix" in 0|1) ;; *) echo "ERROR: --fix must be 0 or 1" >&2; exit 2 ;; esac
    case "$cst_scenario" in touch|grow|chaos) ;; *) echo "ERROR: --scenario must be touch, grow or chaos" >&2; exit 2 ;; esac
    ensure_deps
    if [ -z "$cst_cdp" ]; then
      codeterm plugin browser status 2>/dev/null | grep -q "no browser open" && codeterm plugin browser open >/dev/null
      cst_cdp="$(codeterm plugin browser endpoint | tail -n 1)"
    fi
    case "$cst_cdp" in ws://*) ;; *) echo "ERROR: no browser CDP endpoint; pass --cdp ws://..." >&2; exit 2 ;; esac
    [ -n "$cst_out" ] || cst_out="$(mktemp -d "${TMPDIR:-/tmp}/chat-scroll-trace.XXXXXX")"
    exec {cst_fd}< <(exec node scripts/chat-scroll-trace/serve.mjs)
    cst_server=$!
    trap 'kill "$cst_server" 2>/dev/null || true' EXIT
    cst_url=""
    # The deadline bounds a failed start; readiness is the server's own "ready" line.
    while IFS= read -r -t 120 -u "$cst_fd" cst_line; do
      case "$cst_line" in "ready "*) cst_url="${cst_line#ready }"; break ;; esac
    done
    [ -n "$cst_url" ] || { echo "ERROR: chat-scroll-trace harness server did not report ready" >&2; exit 1; }
    cst_status=0
    node scripts/chat-scroll-trace/trace.mjs --cdp "$cst_cdp" --url "${cst_url}?fix=${cst_fix}&webkit=${cst_webkit}" \
      --strokes "$cst_strokes" --scenario "$cst_scenario" --seed "$cst_seed" --actions "$cst_actions" --out "$cst_out" || cst_status=$?
    echo "chat-scroll-trace: trace.json and screenshots in $cst_out"
    exit "$cst_status"
    ;;
  qa-prep)
    bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "${CT_GATEWAY_OWNER_PID:-$$}" "$CT_GATEWAY_PROCESS_START" "$CMD ${*:2}" -- "${@:2}"
    ;;
  bootstrap)
    ensure_deps
    prepare_bundle_stubs
    cargo build --release --manifest-path cli/Cargo.toml
    CODETERM_DEV=1 node scripts/bundle-cli.js
    echo "Bootstrap complete: bundled CLI is present."
    ;;
  cli-docs)
    cargo build --manifest-path cli/Cargo.toml
    "$(cli_profile_binary debug)" cli-manifest --write
    ;;
  cli)
    cargo build --release --manifest-path cli/Cargo.toml
    built_cli_artifact="$(cli_release_binary)"
    built_cli_hash="$(cli_file_sha256_or_status "$built_cli_artifact")"
    node scripts/bundle-cli.js
    write_cli_docs
    install_cli_to "$CT_DATA_DIR/bin" "$built_cli_artifact" "$built_cli_hash"
    ;;
  clean)
    cargo clean --manifest-path src-tauri/Cargo.toml
    ;;
  kill)
    if named_dev_isolated; then export CARGO_TARGET_DIR="$(named_dev_target_dir)"; fi
    kill_dev
    echo "Done."
    ;;
  dev-server)
    reload_dev_server
    ;;
  dev-prebuilt)
    CT_SPLIT_DEV=1
    if named_dev_isolated; then
      export CARGO_TARGET_DIR="$(named_dev_target_dir)"
    fi
    PREBUILT_LOG="/tmp/codeterm-dev-prebuilt${CT_SUFFIX}.log"
    if [ -n "$DEV_NOTIFY_TAB" ]; then
      : > "$PREBUILT_LOG"
      DEV_NOTIFY_LOG="$PREBUILT_LOG"
    fi
    DEV_NOTIFY_STAGE=preflight
    dev_build_phase provenance-check.begin
    if ! run_prebuilt_preflight validate_dev_provenance; then
      refuse_prebuilt "ERROR: prebuilt dev provenance is absent, stale, or unparseable; refusing before stopping dev." \
        "Run CODETERM_DEV_INSTANCE='$CT_INSTANCE' CODETERM_DEV_API_PORT='$CT_PORT' ./build.sh dev --wait to rebuild and record valid dev provenance."
    fi
    dev_build_phase provenance-check.end
    if ! run_prebuilt_preflight preflight_dev_prebuilt_runtime; then
      refuse_prebuilt "ERROR: prebuilt dev runtime prerequisites are unavailable; refusing before stopping dev." \
        "Run ./build.sh dev --wait to restore prerequisites and record valid dev provenance."
    fi
    dev_build_phase runtime-preflight.end
    stop_dev_before_replacement
    dev_build_phase launch-stop.end
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
    configure_dev_cargo
    export CODETERM_WEB_DIST="$(pwd -P)/dist"
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=$CT_CDP_PORT}"
    # dev-back leaves the frontend to the separate dev-front (Vite HMR) tab.
    if [ "${DEV_FRONT_EXTERNAL:-0}" != 1 ]; then
      start_web_watch
      dev_build_phase watcher-start.end
      start_dev_static_server
      dev_build_phase static-start.end
    fi
    if [ "$CT_DEV_WAIT" -eq 1 ]; then
      DEV_NOTIFY_STAGE=launch
      dev_build_phase native-launch.begin
      if run_outside_tab_job node scripts/dev-prep.mjs --launch-prebuilt \
        --app "$(dev_app_path)" --log "$PREBUILT_LOG" --api-port "$CT_PORT" --record "$(dev_provenance_record)"; then
        PREBUILT_STATUS=0
      else
        PREBUILT_STATUS=$?
      fi
      if [ "$PREBUILT_STATUS" -eq 3 ] && [ "${DEV_FRONT_EXTERNAL:-0}" = 1 ]; then
        trap - EXIT INT TERM
        echo "WARNING: DEV window is still attaching; server and window retained. Inspect $PREBUILT_LOG before recovery." >&2
        exit 3
      fi
      [ "$PREBUILT_STATUS" -eq 0 ] || exit "$PREBUILT_STATUS"
      dev_build_phase native-attachment.end
      HEALTH="$(dev_health_status)"
      EXPECTED_BUILD_SHA="$CODETERM_BUILD_SHA"
      SHA="$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH" server)"
      node scripts/dev-prep.mjs --check-attachment --log "$PREBUILT_LOG" --api-port "$CT_PORT" --listener-pid "$(dev_listener_pid)"
      dev_build_phase readiness-check.end
      echo "Prebuilt dev ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', listener pid $(dev_listener_pid)"
      if [ -n "$DEV_NOTIFY_TAB" ]; then
        dev_notify_ready_after_proof "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CT_INSTANCE" "$CT_PORT" "$SHA" "$(dev_listener_pid)" "$(pwd -P)"
        dev_notify_exit_report 0
      fi
      trap - EXIT INT TERM
    else
      echo "Starting provenance-validated prebuilt dev..."
      if [ -n "$DEV_NOTIFY_TAB" ]; then
        INTERACTIVE_LOG="/tmp/codeterm-dev${CT_SUFFIX}.interactive.log"
        : > "$INTERACTIVE_LOG"
        DEV_NOTIFY_LOG="$INTERACTIVE_LOG"
        EXPECTED_BUILD_SHA="$CODETERM_BUILD_SHA"
        DEV_NOTIFY_STAGE=launch
        DEV_NOTIFY_WAITER_PID=
        exec 3> >(report_read_stream "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$CMD" launch "$INTERACTIVE_LOG" dev)
        DEV_NOTIFY_TEE_PID=$!
        if node scripts/dev-prep.mjs --run-prebuilt --app "$(dev_app_path)" --record "$(dev_provenance_record)" --api-port "$CT_PORT" --log "$INTERACTIVE_LOG" >&3 2>&1; then
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
          if [ "$DEV_NOTIFY_STATUS" -eq 0 ]; then DEV_NOTIFY_STATUS=1; fi
          DEV_NOTIFY_STAGE=exited-before-ready
          dev_notify_failure_once "$DEV_NOTIFY_SENT_MARKER" "$DEV_NOTIFY_TAB" "$DEV_NOTIFY_STAGE" "$CT_INSTANCE" "$CT_PORT" "$INTERACTIVE_LOG" "$DEV_NOTIFY_STATUS"
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
        exec node scripts/dev-prep.mjs --run-prebuilt --app "$(dev_app_path)" --record "$(dev_provenance_record)" --api-port "$CT_PORT" --log "/tmp/codeterm-dev${CT_SUFFIX}.interactive.log"
      fi
    fi
    ;;
  dev-front)
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    ensure_deps
    echo "Vite dev server with HMR on http://127.0.0.1:${CT_VITE_PORT} (DEV API ${CT_PORT})"
    exec env CODETERM_FRONTEND_DEV=1 CODETERM_API_PORT="$CT_PORT" CODETERM_VITE_PORT="$CT_VITE_PORT" \
      node node_modules/vite/bin/vite.js --host 127.0.0.1 --port "$CT_VITE_PORT" --strictPort
    ;;
  dev)
    DEV_NOTIFY_STAGE=preflight
    DEV_NOTIFY_STAGE=build
    if named_dev_isolated; then prepare_named_dev_target; fi
    if dev_window_retainable; then
      export CT_DEV_RETAIN_WINDOW=1
    else
      stop_dev_before_replacement
    fi
    enter_cargo_gateway "$CMD ${*:2}"
    dev_build_phase qa-project.begin
    ct_configure_dev_qa_project
    dev_build_phase qa-project.end
    prepare_dev_frontend_dist
    dev_build_phase frontend-preflight.end
    ensure_deps
    dev_build_phase dependencies.end
    echo "Building CLI..."
    mkdir -p "$(tauri_debug_dir)/.domios-dev-build"
    build_dev_cli "$(tauri_debug_dir)/.domios-dev-build/cli.jsonl"
    # Every dev instance gets its own CLI; production updates its bundled CLI.
    install_cli_to "$CT_DATA_DIR/bin" "$built_cli_artifact" "$built_cli_hash"
    dev_build_phase cli-install.end
    export PATH="$CT_DATA_DIR/bin:$PATH"
    if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
      export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/codeterm"
    else
      export CODETERM_CLI_PATH="$CT_DATA_DIR/bin/codeterm.exe"
    fi
    build_and_launch_dev "${@:2}"

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

    ta_run "rust-unit"  scripts/test-capped.sh --no-default-features --manifest-path src-tauri/Cargo.toml --lib
    ta_run "api-effects" node scripts/check-api-effect-dependencies.mjs
    ta_run "ts-unit"    pnpm -s test:unit
    ta_run "ts-vitest"  node node_modules/vitest/vitest.mjs run
    ta_run "plugin"     pnpm -s test:plugins

    PYTHON_BIN="$(command -v python3 || command -v python)"
    if dev_health_status | node scripts/server-readiness.mjs health-ready; then
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

}
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  codeterm_build_main "$@"
  exit "$?"
else
  codeterm_build_main "$@"
  return "$?"
fi
