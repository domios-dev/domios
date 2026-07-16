#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN="$ROOT/evals/manager-bootstrap/run.sh"
source "$ROOT/evals/manager-bootstrap/helpers.sh"
grep -q 'role.*assistant' "$RUN"
grep -q 'dev_post_as_pane' "$RUN"
grep -q 'timeout first assistant' "$RUN"
grep -q 'exit 7' "$RUN"
! rg -q 'claude|opus|fable|runner\.py' "$ROOT/evals/manager-bootstrap"
python - <<'PY' | first_assistant | grep -qx 'Planner manager-bootstrap-eval not yet established'
import json
chat={"messages":[{"role":"system","content":"Planner"},{"role":"user","content":"who"},{"role":"assistant","content":"Planner manager-bootstrap-eval not yet established"}]}
print(json.dumps(chat))
PY
PROVIDERS='[{"id":"system","system_prompt_delivery":"cli_flag"},{"id":"fallback","system_prompt_delivery":"external"}]'
[ "$(provider_id system <<<"$PROVIDERS")" = system ] && [ "$(provider_id external <<<"$PROVIDERS")" = fallback ]
staff_request system | grep -q '"provider": "system"'
FAKE="$(mktemp -d)"; trap 'rm -rf "$FAKE"' EXIT
printf '#!/usr/bin/env bash\nexit 1\n' > "$FAKE/curl"; chmod +x "$FAKE/curl"
set +e; out="$(PATH="$FAKE:$PATH" bash "$RUN" 2>&1)"; rc=$?; set -e
[ "$rc" -eq 7 ] && grep -q 'dev 7686 unavailable' <<<"$out"
echo "manager-bootstrap offline shape: PASS"
