PARSER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
parser() { python "$PARSER_DIR/parser.py" "$@"; }
first_assistant() { parser first-assistant; }
provider_id() { parser provider-id "$1"; }
new_team_id() { parser team-id "$1"; }
pair_pane_id() { parser field "$1"; }
staffed_pane_id() { parser field staffed-pane; }
mux_absent() { parser mux-absent "$@"; }
spawned_pane_id() { parser field spawned-pane; }
await_first() { local pane="$1" chat answer; chat="$(capture_json dc tab chat --tab "$pane" --json)"; answer="$(first_assistant <<<"$chat")"; [ -z "$answer" ] || { printf '%s' "$answer"; return; }; parser await-event "$pane" 45; }
capture_json() { local output; output="$("$@")" || { echo "manager-bootstrap: JSON command failed: $*" >&2; return 1; }; printf '%s' "$output"; }
staff_request() { python -c 'import json,sys;print(json.dumps({"provider":sys.argv[1],"role":"planner","title":"manager-bootstrap-staffed","task":None,"model":None}))' "$1"; }
