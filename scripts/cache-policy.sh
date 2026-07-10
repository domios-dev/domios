#!/usr/bin/env bash

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

ct_configure_sccache() {
  [ "${CODETERM_USE_SCCACHE:-1}" = "0" ] && return 0
  command -v sccache >/dev/null 2>&1 || {
    echo "[cache-policy] sccache unavailable; building without RUSTC_WRAPPER"
    return 0
  }
  ct_verify_target_dir
  ct_forbid_mutable_cache_dir "${SCCACHE_DIR:-}"
  export RUSTC_WRAPPER=sccache
  [ -n "${SCCACHE_BASEDIRS:-}" ] || export SCCACHE_BASEDIRS="$(ct_repo_root)"
  echo "[cache-policy] sccache verified before build server start"
  echo "[cache-policy] RUSTC_WRAPPER=$RUSTC_WRAPPER"
  echo "[cache-policy] SCCACHE_DIR=${SCCACHE_DIR:-<default>}"
  echo "[cache-policy] SCCACHE_BASEDIRS=${SCCACHE_BASEDIRS}"
  echo "[cache-policy] $(sccache --version)"
  sccache --start-server >/dev/null 2>&1 || true
  sccache --show-stats 2>/dev/null | sed -n '/Cache location/,$p' || true
}

ct_require_prestarted_sccache() {
  command -v sccache >/dev/null 2>&1 || {
    echo "[cache-policy] ERROR: exceptional worktree compilation requires sccache" >&2
    return 2
  }
  ct_forbid_mutable_cache_dir "${SCCACHE_DIR:-}"
  if ! sccache --show-stats >/dev/null 2>&1; then
    echo "[cache-policy] ERROR: shared sccache server is not prestarted" >&2
    echo "[cache-policy] Start it once from the machine integration environment." >&2
    return 2
  fi
  export RUSTC_WRAPPER=sccache
  [ -n "${SCCACHE_BASEDIRS:-}" ] || export SCCACHE_BASEDIRS="$(ct_repo_root)"
  echo "[cache-policy] prestarted shared sccache verified"
  echo "[cache-policy] RUSTC_WRAPPER=$RUSTC_WRAPPER"
  echo "[cache-policy] SCCACHE_DIR=${SCCACHE_DIR:-<default>}"
  echo "[cache-policy] SCCACHE_BASEDIRS=$SCCACHE_BASEDIRS"
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
