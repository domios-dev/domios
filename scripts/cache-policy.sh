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

# SCCACHE_BASEDIRS makes sccache strip a path prefix from source paths before
# hashing, so two builds under different absolute roots can hit the same cache
# entry. That is unsafe for this repo: parallel worktrees share one
# SCCACHE_DIR/server but each has its own (possibly diverged) sources at the
# same relative paths, so a stripped basedir can return an artifact built from
# a different worktree's sources (codeterm#327). Never derive or forward it —
# see essences/build.md#development.
ct_reject_basedir_rewriting() {
  if [ -n "${SCCACHE_BASEDIRS:-}" ]; then
    echo "[cache-policy] WARNING: SCCACHE_BASEDIRS='${SCCACHE_BASEDIRS}' inherited from environment; unsetting." >&2
    echo "[cache-policy] Basedir rewriting is disallowed for this repo: it can hand back a cache hit built from a different worktree's sources. See essences/build.md." >&2
  fi
  unset SCCACHE_BASEDIRS
}

# Cache-key-relevant path scope once basedir rewriting is disabled: sccache
# hashes the real absolute worktree path, so this is what makes two worktrees'
# cache keys diverge. Used by scripts/check-sccache-basedir-isolation.sh.
ct_effective_cache_key_scope() {
  ct_abs_path "$(ct_repo_root)"
}

ct_configure_sccache() {
  [ "${CODETERM_USE_SCCACHE:-1}" = "0" ] && return 0
  command -v sccache >/dev/null 2>&1 || {
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
  echo "[cache-policy] SCCACHE_BASEDIRS=<disabled: per-worktree cache-key isolation, see essences/build.md>"
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
  ct_reject_basedir_rewriting
  export RUSTC_WRAPPER=sccache
  echo "[cache-policy] prestarted shared sccache verified"
  echo "[cache-policy] RUSTC_WRAPPER=$RUSTC_WRAPPER"
  echo "[cache-policy] SCCACHE_DIR=${SCCACHE_DIR:-<default>}"
  echo "[cache-policy] SCCACHE_BASEDIRS=<disabled: per-worktree cache-key isolation, see essences/build.md>"
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
