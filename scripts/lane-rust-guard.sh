#!/usr/bin/env bash
require_canonical_rust_tests() {
  case "$1" in test-one|nextest|check-queue) ;; *) return 0 ;; esac
  local current canonical
  current="$(git rev-parse --show-toplevel 2>/dev/null)" || current=""
  canonical="$(git worktree list --porcelain 2>/dev/null | sed -n 's/^worktree //p' | head -n 1)" || canonical=""
  if [ -n "$current" ] && [ -n "$canonical" ] &&
      node scripts/rust-gate-authorization.mjs "$current" "$canonical" "$@" 2>/dev/null; then
    return 0
  fi
  echo "Rust tests and check-queue run in canonical through the Orchestrator; hand over the selector" >&2
  return 2
}
