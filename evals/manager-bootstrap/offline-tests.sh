#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN="$ROOT/evals/manager-bootstrap/run.sh"
grep -q 'role.*assistant' "$RUN"
grep -q 'dev_post_as_pane' "$RUN"
grep -q 'timeout first assistant' "$RUN"
grep -q 'exit 7' "$RUN"
! rg -q 'claude|opus|fable|runner\.py' "$ROOT/evals/manager-bootstrap"
python - <<'PY'
import json
chat={"messages":[{"role":"system","content":"Planner"},{"role":"user","content":"who"},{"role":"assistant","content":"Planner manager-bootstrap-eval not yet established"}]}
first=next(m["content"] for m in chat["messages"] if m["role"]=="assistant")
assert first.startswith("Planner") and "not yet established" in first
providers=[{"id":"system","system_prompt_delivery":"cli_flag"},{"id":"fallback","system_prompt_delivery":"external"}]
assert next(x["id"] for x in providers if x["system_prompt_delivery"]!="external")=="system"
assert next(x["id"] for x in providers if x["system_prompt_delivery"]=="external")=="fallback"
PY
echo "manager-bootstrap offline shape: PASS"
