#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
SPEC="$ROOT/evals/blueprint-pipeline/echo.yaml"
AUTH="$(dev_auth_header)"
OWNER_TAB="$(dc tab new --json | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("tab_id") or d.get("id"))')"
[[ -n "$OWNER_TAB" ]] || { echo "Could not create the owner tab." >&2; exit 1; }
API="$(dev_api_url)"

coproc BLUEPRINT_EVENTS {
  curl --no-buffer --silent --show-error --include --max-time 60 \
    -H "$AUTH" "$API/api/events/stream"
}
EVENT_PID="$BLUEPRINT_EVENTS_PID"
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
EVENT_CONNECTED=0
while IFS= read -r header <&"${BLUEPRINT_EVENTS[0]}"; do
  if [[ "$header" == *" 200 "* ]]; then
    EVENT_CONNECTED=1
  fi
  if [[ "$header" == $'\r' || -z "$header" ]]; then
    break
  fi
done
if [[ "$EVENT_CONNECTED" != 1 ]]; then
  echo "Blueprint event stream failed to connect." >&2
  exit 1
fi

created="$(dc --as-tab "$OWNER_TAB" blueprint create --spec "$SPEC" --json)"
id="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$created")"
BLUEPRINT_ID="$id"

verify_pipeline() {
  local show log
  show="$(dc --as-tab "$OWNER_TAB" blueprint show "$id" --json)" || return 1
  log="$(dc --as-tab "$OWNER_TAB" blueprint log "$id" --limit 200 --json)" || return 1
  python3 -c '
import json,sys
show=json.loads(sys.argv[1])
log=json.loads(sys.argv[2])
steps=show.get("steps", [])
assert len(steps) == 2 and all(step.get("state") == "done" for step in steps)
rows={entry.get("step_index"): entry for entry in log}
assert all(rows.get(index, {}).get("state") == "done" for index in (0, 1))
' "$show" "$log"
}

if verify_pipeline; then
  echo "Pipeline $id completed both steps."
  exit 0
fi

while IFS= read -r event_line <&"${BLUEPRINT_EVENTS[0]}"; do
  if [[ "$event_line" == data:* ]] && verify_pipeline; then
    echo "Pipeline $id completed both steps."
    exit 0
  fi
done

echo "Pipeline $id did not publish completed steps before the event deadline." >&2
exit 1
