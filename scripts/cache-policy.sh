#!/usr/bin/env bash
# Caller: build.sh dev|dev-prebuilt|kill|e2e|e2e:llm; CI release-linux-arm64.yml; scripts check-sccache-basedir-isolation.sh, check-shared-sccache.sh, dev-prep.mjs; skill build, github-release

ct_repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || pwd
}

ct_abs_path() {
  mkdir -p "$1"
  (cd "$1" && pwd -P)
}

ct_path_under() {
  case "$1" in
    "$2"|"$2"/*) return 0 ;;
    *) return 1 ;;
  esac
}

ct_verify_target_dir() {
  local repo target_abs repo_abs
  repo="$(ct_repo_root)"
  repo_abs="$(ct_abs_path "$repo")"
  if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    echo "[cache-policy] CARGO_TARGET_DIR unset; cargo uses per-worktree manifest targets"
    return 0
  fi
  target_abs="$(ct_abs_path "$CARGO_TARGET_DIR")"
  if ct_path_under "$target_abs" "$repo_abs"; then
    echo "[cache-policy] CARGO_TARGET_DIR isolated: $target_abs"
    return 0
  fi
  if [ "${CODETERM_ALLOW_EXTERNAL_TARGET:-0}" = "1" ]; then
    echo "[cache-policy] external CARGO_TARGET_DIR explicitly allowed: $target_abs"
    return 0
  fi
  echo "[cache-policy] ERROR: external CARGO_TARGET_DIR rejected: $target_abs" >&2
  echo "[cache-policy] Set CODETERM_ALLOW_EXTERNAL_TARGET=1 only for audited QA lanes." >&2
  return 2
}

ct_forbid_mutable_cache_dir() {
  local repo_abs dir_abs
  [ -z "$1" ] && return 0
  repo_abs="$(ct_abs_path "$(ct_repo_root)")"
  dir_abs="$(ct_abs_path "$1")"
  for rel in src-tauri/target cli/target dist src-tauri/gen src-tauri/bundled-cli src-tauri/bundled-embed; do
    if ct_path_under "$dir_abs" "$repo_abs/$rel"; then
      echo "[cache-policy] ERROR: SCCACHE_DIR must not live under mutable artifact path: $dir_abs" >&2
      return 2
    fi
  done
}

# SCCACHE_BASEDIRS makes sccache strip a path prefix from source paths before
# hashing, so two builds under different absolute roots can hit the same cache
# entry. That is unsafe for this repo: parallel worktrees share one
# SCCACHE_DIR/server but each has its own (possibly diverged) sources at the
# same relative paths, so a stripped basedir can return an artifact built from
# a different worktree's sources (codeterm#327). Never derive or forward it —
# see .agents/skills/build/SKILL.md.
ct_reject_basedir_rewriting() {
  if [ -n "${SCCACHE_BASEDIRS:-}" ]; then
    echo "[cache-policy] WARNING: SCCACHE_BASEDIRS='${SCCACHE_BASEDIRS}' inherited from environment; unsetting." >&2
    echo "[cache-policy] Basedir rewriting is disallowed for this repo: it can hand back a cache hit built from a different worktree's sources. See .agents/skills/build/SKILL.md." >&2
  fi
  unset SCCACHE_BASEDIRS
}

# Cache-key-relevant path scope once basedir rewriting is disabled: sccache
# hashes the real absolute worktree path, so this is what makes two worktrees'
# cache keys diverge. Used by scripts/check-sccache-basedir-isolation.sh.
ct_effective_cache_key_scope() {
  ct_abs_path "$(ct_repo_root)"
}

ct_exceptional_target_dir() {
  case "$1" in
    /Users/Shared/domios-ci/work/codeterm|/home/dev/domios-ci/work/codeterm|/home/ci/domios-ci/work/codeterm|/c/domios-ci/work/codeterm)
      printf '%s/target-check\n' "$1"
      ;;
    *) printf '%s/targets/exceptional/%s\n' "$2" "$3" ;;
  esac
}

ct_sccache_secret() (
  if [ -n "${DEV_NOTIFY_PARENT_CLI:-}" ]; then
    unset CODETERM_INSTANCE CODETERM_API_PORT CODETERM_API_URL CODETERM_SOCKET_PATH CODETERM_API_TOKEN CODETERM_DATA_DIR
    if [ "${DEV_NOTIFY_PARENT_CODETERM_API_URL_SET:-}" = x ]; then export CODETERM_API_URL="$DEV_NOTIFY_PARENT_CODETERM_API_URL"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH_SET:-}" = x ]; then export CODETERM_SOCKET_PATH="$DEV_NOTIFY_PARENT_CODETERM_SOCKET_PATH"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_API_TOKEN_SET:-}" = x ]; then export CODETERM_API_TOKEN="$DEV_NOTIFY_PARENT_CODETERM_API_TOKEN"; fi
    if [ "${DEV_NOTIFY_PARENT_CODETERM_DATA_DIR_SET:-}" = x ]; then export CODETERM_DATA_DIR="$DEV_NOTIFY_PARENT_CODETERM_DATA_DIR"; fi
  fi
  "${DEV_NOTIFY_PARENT_CLI:-codeterm}" mem secret get --name "$1" 2>/dev/null
)

ct_sccache_storage() {
  local trace_enabled=0 access_key secret_key
  case "$-" in *x*) trace_enabled=1; set +x ;; esac
  if [ "${CODETERM_SCCACHE_CREDENTIAL_SOURCE:-vault}" = runner ]; then
    access_key="${AWS_ACCESS_KEY_ID:-}"
    secret_key="${AWS_SECRET_ACCESS_KEY:-}"
  else
    access_key="$(ct_sccache_secret domios-sccache-s3-access-key)" || access_key=""
    secret_key="$(ct_sccache_secret domios-sccache-s3-secret-key)" || secret_key=""
  fi
  if [ -n "$access_key" ] && [ -n "$secret_key" ]; then
    export AWS_ACCESS_KEY_ID="$access_key" AWS_SECRET_ACCESS_KEY="$secret_key"
    unset AWS_SESSION_TOKEN
    export SCCACHE_ENDPOINT=http://100.64.10.5:9000 SCCACHE_BUCKET=sccache
    export SCCACHE_REGION=us-east-1 SCCACHE_S3_USE_SSL=false
    export CT_SCCACHE_STORAGE=shared CT_SCCACHE_REASON=""
  else
    unset SCCACHE_ENDPOINT SCCACHE_BUCKET SCCACHE_REGION SCCACHE_S3_USE_SSL
    export CT_SCCACHE_STORAGE=local CT_SCCACHE_REASON="shared credentials unavailable"
  fi
  unset access_key secret_key
  [ "$trace_enabled" = 0 ] || set -x
}

ct_sccache_disk_dir() {
  if [ -n "${SCCACHE_DIR:-}" ]; then
    printf '%s\n' "$SCCACHE_DIR"
    return
  fi
  case "$OSTYPE" in
    darwin*) printf '%s/Library/Caches/Mozilla.sccache\n' "$HOME" ;;
    msys*|cygwin*|win32) printf '%s/Mozilla/sccache/cache\n' "${LOCALAPPDATA:-$HOME/AppData/Local}" ;;
    *) printf '%s/sccache\n' "${XDG_CACHE_HOME:-$HOME/.cache}" ;;
  esac
}

ct_sccache_print_location() {
  if [ "${CT_SCCACHE_STORAGE:-local}" = shared ]; then
    printf 'sccache: shared S3 cache at %s/%s\n' "$SCCACHE_ENDPOINT" "$SCCACHE_BUCKET"
  else
    printf 'sccache: local disk cache at %s (%s; limit %s)\n' \
      "$(ct_sccache_disk_dir)" "${CT_SCCACHE_REASON:-shared credentials unavailable}" "$SCCACHE_CACHE_SIZE"
  fi
}

ct_sccache_stats() {
  if ! ct_sccache_server_live; then
    echo "[cache-policy] ERROR: sccache server is unavailable; offline synthetic counters are not build evidence" >&2
    return 2
  fi
  printf 'sccache endpoint: '
  node "$(ct_repo_root)/scripts/sccache-server-probe.mjs" --address
  sccache --show-stats
}

ct_sccache_server_live() {
  node "$(ct_repo_root)/scripts/sccache-server-probe.mjs"
}

ct_sccache_status() {
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-10G}"
  ct_sccache_storage
  ct_sccache_print_location
  local stats location http_status
  stats=""
  if ct_sccache_server_live; then stats="$(sccache --show-stats 2>/dev/null)" || stats=""; fi
  location="$(printf '%s\n' "$stats" | sed -n '/^Cache location/p')"
  if [ -n "$location" ]; then
    printf 'running server: %s\n' "$location"
  else
    printf 'running server: cache location unavailable\n'
  fi
  http_status="$(curl --silent --output /dev/null --write-out '%{http_code}' --max-time 3 \
    http://100.64.10.5:9000/minio/health/live 2>/dev/null)" || http_status="unreachable"
  printf 'shared endpoint health: %s\n' "$http_status"
  if [ "${1:-}" = verify ]; then
    if [ "$CT_SCCACHE_STORAGE" != shared ] ||
        ! ct_sccache_storage_matches "$(printf '%s\n' "$stats" | sed -n 's/^Cache location[[:space:]]*//p')"; then
      echo "[cache-policy] ERROR: shared S3 cache is not configured and running; provision it while idle" >&2
      return 2
    fi
  fi
}

ct_sccache_verify() {
  ct_sccache_status verify
}

ct_sccache_storage_matches() {
  if [ "$CT_SCCACHE_STORAGE" = shared ]; then
    case "$1" in
      "s3, name: $SCCACHE_BUCKET, prefix: "*) return 0 ;;
      *) return 1 ;;
    esac
  else
    case "$1" in
      'Local disk:'*|'s3, name: '*) return 0 ;;
      *) return 1 ;;
    esac
  fi
}

ct_sccache_restart() {
  if command -v pgrep >/dev/null 2>&1 && { pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1; }; then
    echo "[cache-policy] ERROR: cache configuration requires an idle machine; Cargo or rustc is running" >&2
    return 75
  fi
  if command -v cygpath >/dev/null 2>&1 &&
      powershell.exe -NoProfile -NonInteractive -Command 'if (Get-Process cargo,rustc -ErrorAction SilentlyContinue) { exit 0 }; exit 1' >/dev/null 2>&1; then
    echo "[cache-policy] ERROR: cache configuration requires an idle machine; Cargo or rustc is running" >&2
    return 75
  fi
  sccache --stop-server >/dev/null 2>&1 || true
  if ! sccache --start-server >/dev/null 2>&1; then
    echo "[cache-policy] ERROR: sccache server failed to start with $CT_SCCACHE_STORAGE storage" >&2
    return 2
  fi
}

ct_configure_sccache() {
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-10G}"
  if [ "${CODETERM_USE_SCCACHE:-1}" = 0 ] && [ "${CODETERM_SCCACHE_VERIFY_ONLY:-0}" != 1 ]; then return 0; fi
  command -v sccache >/dev/null 2>&1 || {
    if [ "${CODETERM_SCCACHE_VERIFY_ONLY:-0}" = 1 ]; then
      echo "[cache-policy] ERROR: shared cache verification requires sccache" >&2
      return 2
    fi
    echo "[cache-policy] sccache unavailable; building without RUSTC_WRAPPER"
    return 0
  }
  ct_verify_target_dir
  ct_forbid_mutable_cache_dir "${SCCACHE_DIR:-}"
  ct_reject_basedir_rewriting
  export RUSTC_WRAPPER=sccache
  echo "[cache-policy] sccache verified before build server start"
  echo "[cache-policy] RUSTC_WRAPPER=$RUSTC_WRAPPER"
  echo "[cache-policy] SCCACHE_DIR=${SCCACHE_DIR:-<default>}"
  echo "[cache-policy] SCCACHE_BASEDIRS=<disabled: per-worktree cache-key isolation, see .agents/skills/build/SKILL.md>"
  echo "[cache-policy] $(sccache --version)"
  if [ "${CODETERM_SCCACHE_VERIFY_ONLY:-0}" = 1 ]; then
    ct_sccache_verify
    return $?
  fi
  local stats location
  stats=""
  if ct_sccache_server_live; then stats="$(sccache --show-stats 2>/dev/null)" || stats=""; fi
  location="$(printf '%s\n' "$stats" | sed -n 's/^Cache location[[:space:]]*//p')"
  ct_sccache_storage
  if [ "$CT_SCCACHE_STORAGE" = local ] && [[ "$location" == 's3, name: '* ]]; then
    local running_bucket="${location#s3, name: }"
    running_bucket="${running_bucket%%, prefix:*}"
    printf 'sccache: shared S3 cache at http://100.64.10.5:9000/%s (using running server credentials)\n' "$running_bucket"
  else
    ct_sccache_print_location
  fi
  if ct_sccache_storage_matches "$location"; then
    echo "[cache-policy] reusing running sccache server"
  else
    ct_sccache_restart || return $?
    if ! ct_sccache_server_live; then
      echo "[cache-policy] ERROR: sccache startup did not leave a live server" >&2
      return 2
    fi
    stats="$(sccache --show-stats 2>/dev/null)" || return 2
  fi
  printf '%s\n' "$stats" | sed -n '/Cache location/,$p'
}

ct_require_prestarted_sccache() {
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-10G}"
  command -v sccache >/dev/null 2>&1 || {
    echo "[cache-policy] ERROR: exceptional worktree compilation requires sccache" >&2
    return 2
  }
  ct_forbid_mutable_cache_dir "${SCCACHE_DIR:-}"
  if ! ct_sccache_server_live || ! sccache --show-stats >/dev/null 2>&1; then
    echo "[cache-policy] ERROR: shared sccache server is not prestarted" >&2
    echo "[cache-policy] Start it once from the machine integration environment." >&2
    return 2
  fi
  ct_reject_basedir_rewriting
  export RUSTC_WRAPPER=sccache
  echo "[cache-policy] prestarted shared sccache verified"
  echo "[cache-policy] RUSTC_WRAPPER=$RUSTC_WRAPPER"
  echo "[cache-policy] SCCACHE_DIR=${SCCACHE_DIR:-<default>}"
  echo "[cache-policy] SCCACHE_BASEDIRS=<disabled: per-worktree cache-key isolation, see .agents/skills/build/SKILL.md>"
  echo "[cache-policy] $(sccache --version)"
}

ct_host_triple() {
  rustc -vV | awk -F': ' '/^host:/{print $2}'
}

ct_write_provenance() {
  node scripts/cache-provenance.mjs write \
    --kind "$1" --profile "$2" --target "$3" --flags "${4:-}" --features "${5:-}" \
    --artifact "$6" --out "$7"
}

ct_snapshot_provenance() {
  node scripts/cache-provenance.mjs snapshot \
    --kind "$1" --profile "$2" --target "$3" --flags "${4:-}" --features "${5:-}" \
    --out "$6"
}

ct_check_provenance() {
  node scripts/cache-provenance.mjs check \
    --kind "$1" --profile "$2" --target "$3" --flags "${4:-}" --features "${5:-}" \
    --manifest "$6"
}

ct_cache_retention() {
  local dir bash_exe self_pid result=0
  dir="$(ct_gateway_dir)"
  bash_exe="$BASH"
  if command -v cygpath >/dev/null 2>&1; then bash_exe="$(cygpath -w "$bash_exe")"; fi
  if ! mkdir "$dir/state.lock" 2>/dev/null; then
    echo "[cache-policy] gateway metadata is busy; retention was not started" >&2
    return 75
  fi
  self_pid="${CT_GATEWAY_MUTEX_PID:-$$}"
  printf '%s\n' "$self_pid" > "$dir/state.lock/pid"
  CT_GATEWAY_MUTEX_START="$(ct_gateway_process_start_token "$self_pid")"
  printf '%s\n' "$CT_GATEWAY_MUTEX_START" > "$dir/state.lock/start_token"
  if [ -z "$CT_GATEWAY_MUTEX_START" ]; then
    ct_gateway_mutex_release "$dir"
    echo "[cache-policy] cannot identify retention owner; no cache paths changed" >&2
    return 2
  fi
  CT_CACHE_RETENTION_LOCK_HELD=1 CT_CACHE_BASH="$bash_exe" \
    node "$(ct_repo_root)/scripts/cache-retention.mjs" "$1" "$dir" "$(pwd -P)" "${2:-}" || result=$?
  ct_gateway_mutex_release "$dir"
  return "$result"
}

ct_mark_exceptional_target() {
  ct_cache_retention mark "$CARGO_TARGET_DIR"
}
