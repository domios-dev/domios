#!/usr/bin/env bash
# Behaviour assertions for the shared role-preset seam. Sourced by the benches
# that already own a scenario (creation, lifecycle, binding, bootstrap, GA), so
# each bench proves the seam in its own context instead of duplicating setup.
#
# Requires evals/_lib/lib.sh to have been sourced first: every daemon call here
# goes through `dc`, which is locked to the dev instance.

set -u

# Full JSON view for one role type, optionally with a task persona.
preset_show() {
  local role_type="$1" guidance="${2:-}"
  local args=( role show "$role_type" --json )
  [ -n "$guidance" ] && args+=( --guidance "$guidance" )
  dc "${args[@]}"
}

preset_field() {
  local key="$1"
  python3 -c "import json,sys; print(json.load(sys.stdin)['result'].get('$key',''))"
}

# Exact text one occasion emits, straight from the shared resolver.
preset_occasion_text() {
  local occasion="$1"
  python3 -c "
import json, sys
view = json.load(sys.stdin)['result']
for entry in view.get('occasions', []):
    if entry.get('occasion') == '$occasion':
        sys.stdout.write(entry.get('text', ''))
        raise SystemExit(0)
raise SystemExit('occasion $occasion missing from the rendered view')
"
}

preset_occasion_emission() {
  local occasion="$1"
  python3 -c "
import json, sys
view = json.load(sys.stdin)['result']
for entry in view.get('occasions', []):
    if entry.get('occasion') == '$occasion':
        print(entry.get('emission', ''))
        raise SystemExit(0)
raise SystemExit('occasion $occasion missing from the rendered view')
"
}

preset_fragment_sources() {
  python3 -c "
import json, sys
view = json.load(sys.stdin)['result']
print(' '.join(f.get('source', '') for f in view.get('fragments', [])))
"
}

# Every builtin manager type resolves through one resolver from the default
# package and bootstraps with non-empty guidance.
assert_manager_kinds_resolve() {
  local kind view text source
  for kind in orchestrator planner watcher cartographer curator; do
    view="$(preset_show "$kind")"
    [ "$(preset_field type <<<"$view")" = "$kind" ] || {
      echo "role show did not resolve type $kind" >&2; return 1; }
    source="$(python3 -c "import json,sys; print(json.load(sys.stdin)['result'].get('provenance',{}).get('source',''))" <<<"$view")"
    [ "$source" = builtin ] || {
      echo "$kind lost its builtin provenance" >&2; return 1; }
    text="$(preset_occasion_text manager_bootstrap <<<"$view")"
    [ -n "$text" ] || { echo "$kind bootstraps with no guidance" >&2; return 1; }
  done
}

# A Planner keeps Team authority and a Worker never gains it.
assert_planner_team_authority() {
  local listing
  listing="$(dc role list --json)"
  python3 - "$listing" <<'PY'
import json, sys
roles = {r["type"]: r for r in json.loads(sys.argv[1])["result"]["roles"]}
assert roles["planner"]["team_authority"], "planner lost Team authority"
assert not roles["planner"]["anchors_team"], "planner must not anchor a Team"
assert roles["orchestrator"]["anchors_team"], "orchestrator lost anchoring"
assert roles["orchestrator"]["can_spawn"], "orchestrator lost spawning"
assert roles["watcher"]["host_only"], "watcher must stay host-only"
for role in roles.values():
    if role["group"] == "worker":
        assert not (role["anchors_team"] or role["team_authority"] or role["can_spawn"]), \
            f"worker type {role['type']} minted capability"
PY
}

# A dispatched Worker's personality reaches the launch prompt as the preset's
# task overlay, and never leaks into the assignment contract slot.
assert_worker_task_preset_layering() {
  local persona="You are Mira, extending the shared preset seam."
  local view identity contract sources
  view="$(preset_show qa "$persona")"
  identity="$(preset_occasion_text worker_launch_identity <<<"$view")"
  [ "$identity" = "$persona" ] || {
    echo "worker identity slot did not carry the task persona: $identity" >&2; return 1; }
  contract="$(preset_occasion_text launch_assignment <<<"$view")"
  case "$contract" in
    *"$persona"*) echo "task persona duplicated into the assignment slot" >&2; return 1;;
  esac
  [ -n "$contract" ] || { echo "worker assignment slot lost its contract" >&2; return 1; }
  sources="$(preset_fragment_sources <<<"$view")"
  case "$sources" in
    builtin*host*task_overlay) ;;
    *) echo "unexpected worker fragment order: $sources" >&2; return 1;;
  esac
}

