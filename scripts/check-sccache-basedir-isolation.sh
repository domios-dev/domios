#!/usr/bin/env bash
# Containment check for codeterm#327: assert that SCCACHE_BASEDIRS can never
# make two worktrees' sccache cache keys collide.
#
# This does NOT reproduce the original bad-artifact incident — nobody has
# been able to reproduce it on demand (see .agents/skills/build/SKILL.md). It asserts the
# derived configuration directly: no compiling, no sccache server, no
# network. Pass/fail is a pure function of scripts/cache-policy.sh and
# scripts/test-capped.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/cache-policy.sh
source "$ROOT/scripts/cache-policy.sh"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

# --- 1. Static containment: the two SCCACHE_BASEDIRS default/export sites
#        that caused codeterm#327 must be gone, and both sccache entry
#        points must run the rejection guard. ------------------------------

if grep -Eq 'export SCCACHE_BASEDIRS="\$\(ct_repo_root\)"' "$ROOT/scripts/cache-policy.sh"; then
  fail "scripts/cache-policy.sh still defaults SCCACHE_BASEDIRS to the repo root"
fi
if grep -Eq 'SCCACHE_BASEDIRS="\$\{SCCACHE_BASEDIRS:-\$ROOT\}"' "$ROOT/scripts/test-capped.sh"; then
  fail "scripts/test-capped.sh still defaults SCCACHE_BASEDIRS to the worktree root"
fi

awk '/^ct_configure_sccache\(\)/,/^}/' "$ROOT/scripts/cache-policy.sh" | grep -q 'ct_reject_basedir_rewriting' \
  || fail "ct_configure_sccache no longer calls ct_reject_basedir_rewriting"
awk '/^ct_require_prestarted_sccache\(\)/,/^}/' "$ROOT/scripts/cache-policy.sh" | grep -q 'ct_reject_basedir_rewriting' \
  || fail "ct_require_prestarted_sccache no longer calls ct_reject_basedir_rewriting"

echo "OK static: no worktree-root SCCACHE_BASEDIRS default remains; both sccache entry points reject it"

# --- 2. Dynamic containment: two distinct worktree roots must derive two
#        distinct cache-key scopes, and a leaked/inherited SCCACHE_BASEDIRS
#        must never survive ct_reject_basedir_rewriting. ------------------

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

WT_A="$WORK/worktree-a"
WT_B="$WORK/worktree-b"
mkdir -p "$WT_A" "$WT_B"
(cd "$WT_A" && git init -q)
(cd "$WT_B" && git init -q)

scope_for() {
  local worktree="$1"
  (
    cd "$worktree"
    # Simulate the pre-fix hazard: a caller (or a leaked parent-shell var)
    # sets SCCACHE_BASEDIRS to this worktree's own root, exactly what the
    # removed `[ -n ... ] || export SCCACHE_BASEDIRS=...` lines used to do.
    export SCCACHE_BASEDIRS="$worktree"
    source "$ROOT/scripts/cache-policy.sh"
    ct_reject_basedir_rewriting
    [ -z "${SCCACHE_BASEDIRS:-}" ] || fail "SCCACHE_BASEDIRS survived ct_reject_basedir_rewriting in $worktree"
    ct_effective_cache_key_scope
  )
}

SCOPE_A="$(scope_for "$WT_A")"
SCOPE_B="$(scope_for "$WT_B")"

[ -n "$SCOPE_A" ] || fail "empty cache-key scope derived for worktree A"
[ -n "$SCOPE_B" ] || fail "empty cache-key scope derived for worktree B"
[ "$SCOPE_A" != "$SCOPE_B" ] || fail "worktree A and B derived the SAME cache-key scope ($SCOPE_A) — this is exactly the codeterm#327 collision"

echo "OK dynamic: worktree A scope=$SCOPE_A"
echo "OK dynamic: worktree B scope=$SCOPE_B"
echo "OK dynamic: scopes differ — a cache hit in one worktree cannot key-match the other"

echo "PASS scripts/check-sccache-basedir-isolation.sh"
