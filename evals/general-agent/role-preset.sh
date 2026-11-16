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

# The General Agent composes its own instructions file and is never launched by role.
ga_type_refused() { ! dc role show general_agent --json >/dev/null 2>&1; }
check "general_agent is refused as a role type" ga_type_refused

# Team registrations never turn a worker type into something that anchors or spawns.
check "no worker type claims capability" assert_planner_team_authority

[ "$FAILED" -eq 0 ] || { echo "RESULT: FAIL ($FAILED)" >&2; exit 1; }
echo "RESULT: general-agent role-preset seam intact"
