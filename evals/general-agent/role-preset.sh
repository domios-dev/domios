#!/usr/bin/env bash
# The General Agent is an anchoring Manager kind like any other: it must resolve
# through the shared role-preset resolver, keep its anchoring capability, and be
# reachable by the inspection surface with builtin provenance.
#
# Usage: bash evals/general-agent/role-preset.sh
# Exit:  0 pass, 1 assertion failed, 7 dev unavailable.
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/role_preset.sh"

dev_alive || { echo "dev 7686 unavailable" >&2; exit 7; }

FAILED=0
check() {
  local label="$1"; shift
  if "$@"; then echo "  ok   $label"; else echo "  FAIL $label" >&2; FAILED=$((FAILED + 1)); fi
}

# The GA kind reaches the resolver and keeps its anchoring capability.
check "manager kinds resolve through one resolver" assert_manager_kinds_resolve
check "team authority and anchoring unchanged" assert_planner_team_authority
check "unmatched pairs use the declared fallback" assert_declared_fallback

# The GA composes its own instructions file, so it carries no bundled role
# suffix — but its bootstrap slot must still fall back to declared text, never
# to an empty identity.
GA_VIEW="$(preset_show Manager general_agent)"
check "general_agent renders with builtin provenance" \
  test "$(preset_field origin <<<"$GA_VIEW")" = builtin
check "general_agent bootstrap text is non-empty" \
  test -n "$(preset_occasion_text manager_bootstrap <<<"$GA_VIEW")"
check "general_agent contributes no launch-assignment suffix" \
  test -z "$(preset_occasion_text launch_assignment <<<"$GA_VIEW")"

# An overlay file can never turn the machine agent into something spawnable.
check "no overlay kind claims capability" assert_planner_team_authority

[ "$FAILED" -eq 0 ] || { echo "RESULT: FAIL ($FAILED)" >&2; exit 1; }
echo "RESULT: general-agent role-preset seam intact"
