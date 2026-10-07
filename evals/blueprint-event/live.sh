#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
if [[ "${CODETERM_INSTANCE:-}" != "bpdev" ]]; then
  echo "Set CODETERM_INSTANCE=bpdev before running this event fixture." >&2
  exit 2
fi
source "$ROOT/evals/_lib/lib.sh"
PLUGIN="$ROOT/evals/blueprint-event/fixture-plugin"
SPEC="$ROOT/evals/blueprint-event/pipeline.yaml"
TRANSCRIPT="$ROOT/evals/blueprint-event/transcript.md"
AUTH="$(dev_auth_header)"
OWNER_TAB="$(dc tab new --json | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("tab_id") or d.get("id"))')"
[[ -n "$OWNER_TAB" ]] || { echo "Could not create the owner tab." >&2; exit 1; }
API="$(dev_api_url)"

coproc GRAPH_EVENTS {
  curl --no-buffer --silent --show-error --include --max-time 60 \
    -H "$AUTH" "$API/api/events/stream"
}
EVENT_PID="$GRAPH_EVENTS_PID"
BLUEPRINT_ID=""
cleanup() {
  if [[ -n "$BLUEPRINT_ID" ]]; then
    dc --as-tab "$OWNER_TAB" blueprint stop "$BLUEPRINT_ID" --json >/dev/null 2>&1 || true
  fi
  dc tab close --tab "$OWNER_TAB" >/dev/null 2>&1 || true
  if [[ -n "$EVENT_PID" ]]; then
    kill "$EVENT_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

connected=0
while IFS= read -r header <&"${GRAPH_EVENTS[0]}"; do
  if [[ "$header" == *" 200 "* ]]; then connected=1; fi
  if [[ "$header" == $'\r' || -z "$header" ]]; then break; fi
done
[[ "$connected" == 1 ]] || { echo "Graph event stream did not connect." >&2; exit 1; }

dc plugin install --from-dir "$PLUGIN" --json >/dev/null
dc plugin config fixture-plugin --grant graph_events --json >/dev/null
source_ready=0
while IFS= read -r event_line <&"${GRAPH_EVENTS[0]}"; do
  if [[ "$event_line" == data:* ]]; then
    nodes="$(dc --as-tab "$OWNER_TAB" blueprint nodes --json)"
    if python3 -c 'import json,sys; assert any(node.get("kind")=="event_source" and node.get("type")=="fixture.message" for node in json.loads(sys.argv[1]))' "$nodes"; then
      source_ready=1
      break
    fi
  fi
done
[[ "$source_ready" == 1 ]] || { echo "Fixture event source did not become available." >&2; exit 1; }
created="$(dc --as-tab "$OWNER_TAB" blueprint create --spec "$SPEC" --json)"
BLUEPRINT_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$created")"

verify_result() {
  local show log
  show="$(dc --as-tab "$OWNER_TAB" blueprint show "$BLUEPRINT_ID" --json)" || return 1
  log="$(dc --as-tab "$OWNER_TAB" blueprint log "$BLUEPRINT_ID" --limit 200 --json)" || return 1
  python3 -c '
import json,sys
show=json.loads(sys.argv[1])
log=json.loads(sys.argv[2])
assert show.get("event_subscription", {}).get("delivered") == 1
assert show.get("result", {}).get("state") == "delivered"
runs={entry.get("run_id") for entry in log if entry.get("step_name") == "$run" and entry.get("state") == "done"}
assert len(runs) == 1, runs
assert all(entry.get("step_name") != "ask" for entry in log)
print(json.dumps({"blueprint":show, "journal":log}, indent=2))
' "$show" "$log" >"$TRANSCRIPT"
}

if verify_result; then
  echo "Fixture event produced one delivered, model-free pipeline run; transcript: $TRANSCRIPT"
  exit 0
fi
while IFS= read -r event_line <&"${GRAPH_EVENTS[0]}"; do
  if [[ "$event_line" == data:* ]] && verify_result; then
    echo "Fixture event produced one delivered, model-free pipeline run; transcript: $TRANSCRIPT"
    exit 0
  fi
done
echo "Fixture event did not deliver a completed pipeline result before the event deadline." >&2
exit 1
