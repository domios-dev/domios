#!/usr/bin/env bash
# Behaviour assertions for the shared role-preset seam. Sourced by the benches
# that already own a scenario (creation, lifecycle, binding, bootstrap, GA), so
# each bench proves the seam in its own context instead of duplicating setup.
#
# Requires evals/_lib/lib.sh to have been sourced first: every daemon call here
# goes through `dc`, which is locked to the dev instance.

set -u

# Full JSON view for one (role, kind, task-persona) triple.
preset_show() {
  local role="$1" kind="${2:-}" guidance="${3:-}"
  local args=( persona show --role "$role" --json )
  [ -n "$kind" ] && args+=( --kind "$kind" )
  [ -n "$guidance" ] && args+=( --guidance "$guidance" )
  dc "${args[@]}"
}

preset_field() {
  local key="$1"
  python -c "import json,sys; print(json.load(sys.stdin).get('$key',''))"
}

# Exact text one occasion emits, straight from the shared resolver.
preset_occasion_text() {
  local occasion="$1"
  python -c "
import json, sys
view = json.load(sys.stdin)
for entry in view.get('occasions', []):
    if entry.get('occasion') == '$occasion':
        sys.stdout.write(entry.get('text', ''))
        raise SystemExit(0)
raise SystemExit('occasion $occasion missing from the rendered view')
"
}

preset_occasion_emission() {
  local occasion="$1"
  python -c "
import json, sys
view = json.load(sys.stdin)
for entry in view.get('occasions', []):
    if entry.get('occasion') == '$occasion':
        print(entry.get('emission', ''))
        raise SystemExit(0)
raise SystemExit('occasion $occasion missing from the rendered view')
"
}

preset_fragment_sources() {
  python -c "
import json, sys
view = json.load(sys.stdin)
print(' '.join(f.get('source', '') for f in view.get('fragments', [])))
"
}

# Every builtin manager kind resolves through one resolver and bootstraps with
# non-empty guidance. Covers the manager-bootstrap and General Agent surfaces.
assert_manager_kinds_resolve() {
  local kind view text
  for kind in orchestrator planner watcher general_agent; do
    view="$(preset_show Manager "$kind")"
    [ "$(preset_field kind_id <<<"$view")" = "$kind" ] || {
      echo "persona show did not resolve kind $kind" >&2; return 1; }
    [ "$(preset_field origin <<<"$view")" = builtin ] || {
      echo "$kind lost its builtin origin" >&2; return 1; }
    text="$(preset_occasion_text manager_bootstrap <<<"$view")"
    [ -n "$text" ] || { echo "$kind bootstraps with no guidance" >&2; return 1; }
  done
}

# A Planner keeps Team authority and a Worker never gains it.
assert_planner_team_authority() {
  local listing
  listing="$(dc persona list --json)"
  python - "$listing" <<'PY'
import json, sys
kinds = {k["kind_id"]: k for k in json.loads(sys.argv[1])["kinds"]}
assert kinds["planner"]["team_authority"], "planner lost Team authority"
assert not kinds["planner"]["anchors_team"], "planner must not anchor a Team"
assert kinds["orchestrator"]["anchors_team"], "orchestrator lost anchoring"
assert kinds["orchestrator"]["can_spawn"], "orchestrator lost spawning"
assert kinds["general_agent"]["anchors_team"], "general_agent lost anchoring"
assert not kinds["worker"]["team_authority"], "worker gained Team authority"
assert not kinds["worker"]["can_spawn"], "worker gained spawning"
for kind in kinds.values():
    if kind["origin"] == "overlay":
        assert not (kind["anchors_team"] or kind["team_authority"] or kind["can_spawn"]), \
            f"overlay kind {kind['kind_id']} minted capability"
PY
}

# A dispatched Worker's personality reaches the launch prompt as the preset's
# task overlay, and never leaks into the assignment contract slot.
assert_worker_task_preset_layering() {
  local persona="You are Mira, extending the shared preset seam."
  local view identity contract sources
  view="$(preset_show Worker rust-backend-dev "$persona")"
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
    *inherited_base*task_overlay*) ;;
    *) echo "unexpected worker fragment order: $sources" >&2; return 1;;
  esac
}

# An unmatched pair lands on the occasion's declared fallback, never on silence.
assert_declared_fallback() {
  local view
  view="$(preset_show Manager field-marshal)"
  [ "$(preset_occasion_emission manager_bootstrap <<<"$view")" = declared_fallback ] || {
    echo "unmatched manager kind did not use the declared fallback" >&2; return 1; }
  [ -n "$(preset_occasion_text manager_bootstrap <<<"$view")" ] || {
    echo "declared fallback is empty" >&2; return 1; }
  [ "$(preset_field origin <<<"$view")" = unregistered ] || {
    echo "an unknown kind must not claim builtin provenance" >&2; return 1; }
}

# The whole seam in one call, for benches that just want the guard.
assert_role_preset_seam() {
  assert_manager_kinds_resolve || return 1
  assert_planner_team_authority || return 1
  assert_worker_task_preset_layering || return 1
  assert_declared_fallback || return 1
}
