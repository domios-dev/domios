#!/usr/bin/env bash
require_canonical_rust_tests() {
  case "$1" in test-one|nextest|check-queue) ;; *) return 0 ;; esac
  [ "${CODETERM_ALLOW_LANE_RUST_TESTS:-0}" != 1 ] || return 0
  local current canonical
  current="$(git rev-parse --show-toplevel)" || return 2
  canonical="$(git worktree list --porcelain | sed -n 's/^worktree //p' | head -n 1)" || return 2
  if [ "$current" != "$canonical" ]; then
    echo "Rust tests and check-queue run in canonical through the Orchestrator; hand over the selector" >&2
    return 2
  fi
}
