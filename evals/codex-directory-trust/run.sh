#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
HEAD="$(git rev-parse HEAD)"

health="$(dev_get /api/health)"
printf '%s' "$health" | python - "$HEAD" <<'PY'
import json,sys
h=json.load(sys.stdin); want=sys.argv[1]
assert h.get("build")==want, (h.get("build"),want)
PY

panes=""
cleanup() {
  for pane in $panes; do dev_post "/api/panes/$pane/close" '{}' >/dev/null 2>&1 || true; done
  rm -rf "$foreign"
}
trap cleanup EXIT

spawn() {
  dc agent spawn "$@" --json | python -c 'import json,sys;print(json.load(sys.stdin)["pane_id"])'
}
pane() {
  dev_get /api/panes | python - "$1" <<'PY'
import json,sys
for pane in json.load(sys.stdin):
 if pane["id"]==sys.argv[1]: print(json.dumps(pane)); break
PY
}
assert_ready() {
  pane "$1" | python - "$2" <<'PY'
import json,sys
p=json.load(sys.stdin); cwd=sys.argv[1]
assert p.get("cwd")==cwd, p
assert p.get("pending_prompt") is None, p
assert p.get("agent_type"), p
PY
}

foreign="$ROOT/../foreign-lookalike-lane"
mkdir -p "$foreign"
positive_a="$(spawn codex --role Worker --title ct-trust-positive-a)"
positive_b="$(spawn codex --role Worker --title ct-trust-positive-b)"
foreign_pane="$(spawn codex --cwd "$foreign" --title ct-trust-foreign)"
panes="$positive_a $positive_b $foreign_pane"

lane_a="$(pane "$positive_a" | python -c 'import json,sys;print(json.load(sys.stdin)["cwd"])')"
lane_b="$(pane "$positive_b" | python -c 'import json,sys;print(json.load(sys.stdin)["cwd"])')"
[ "$lane_a" != "$lane_b" ]
assert_ready "$positive_a" "$lane_a"
assert_ready "$positive_b" "$lane_b"
pane "$foreign_pane" | python - "$foreign" <<'PY'
import json,sys
p=json.load(sys.stdin)
assert p.get("cwd")==sys.argv[1], p
assert p.get("pending_prompt") is not None, p
PY

printf 'PASS codex-directory-trust %s %s %s\n' "$positive_a" "$positive_b" "$foreign_pane"
