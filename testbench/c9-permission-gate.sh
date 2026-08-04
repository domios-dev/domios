#!/usr/bin/env bash
# C9 deterministic gate (Option A) — real opencode permission-policy trigger.
#
# Proves the FULL structured-first roundtrip, not just a server connection:
#   tool-execution prompt -> opencode permission.asked -> SSE subscriber ->
#   tab.pending_prompt set -> agent_state.state == "question" -> POST
#   /prompt/respond {value} -> pending_prompt cleared -> state leaves question.
#
# REQUIRES opencode launched with a permission policy that ASKS (Option A; Lead
# wires this out-of-the-box). If opencode auto-approves, no permission.asked is
# emitted and this gate reports NO-TRIGGER (provider config issue, not SSE failure).
#
# state==question can ONLY come from a structured picker (live classifier step 1
# picker_question); the visible-cursor grid can't produce it. pending_prompt.options
# present == the structured event really arrived via the subscriber.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
source evals/_lib/lib.sh 2>/dev/null; export PATH="/usr/bin:/bin:$PATH"
TOK=$(python3 -c "import re;print(re.search(r'token\s*=\s*\"([^\"]+)\"',open('$HOME/.codeterm-dev/config.toml').read()).group(1))")
post(){ /usr/bin/curl -s -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' --data "$2" "http://127.0.0.1:7686$1"; }
get(){ /usr/bin/curl -s -H "Authorization: Bearer $TOK" "http://127.0.0.1:7686$1"; }

TAB=""; SPAWNED_WS=""
PROMPT="Run the shell command 'echo codeterm-c9-probe' using your tool."
while [ "$#" -gt 0 ]; do case "$1" in --tab) TAB="$2"; shift 2;; --prompt) PROMPT="$2"; shift 2;; *) echo "unknown $1"; exit 2;; esac; done

# tab snapshot helper: prints "STATE|HAS_PENDING|FIRST_OPTION_VALUE"
snap(){ get /api/tabs | python3 -c "
import sys,json
d=json.load(sys.stdin)
p=next((x for x in d if x['id']=='$1'),None)
if not p: print('?|0|'); sys.exit()
st=(p.get('agent_state') or {}).get('state')
pp=p.get('pending_prompt')
opt=''
if pp and pp.get('options'): opt=str(pp['options'][0].get('value', pp['options'][0].get('label','')))
print(f\"{st}|{1 if pp else 0}|{opt}\")"; }

if [ -z "$TAB" ]; then
  SPAWNED_WS=$(dc workspace create "VIS-c9gate" --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
  trap 'dc workspace close "$SPAWNED_WS" >/dev/null 2>&1 || true' EXIT
  TAB=$(dc agent spawn opencode --workspace "$SPAWNED_WS" --title c9gate --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin).get('tab_id',''))")
  echo "spawned opencode tab=$TAB"
  python3 -c 'import time;time.sleep(9)'
fi

# settle idle baseline; never press Enter here, or the gate can choose a default
# option before the explicit structured respond step.
for i in $(seq 1 40); do
  IFS='|' read -r s h o <<<"$(snap "$TAB")"
  if [ "$s" = question ]; then
    echo "RESULT: BASELINE-BLOCKED — tab already has a pending question; refusing to auto-answer"
    exit 2
  fi
  [ "$s" = idle ] && break
  python3 -c 'import time;time.sleep(0.5)'
done
echo "baseline=$(snap "$TAB")"

echo "== sending tool-execution prompt (should trip permission policy) =="
post "/api/tabs/$TAB/send" "$(python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"enter":True}))' "$PROMPT")" >/dev/null

echo "== watching for structured permission question (pending_prompt + state=question) =="
val=""; saw=0
for i in $(seq 1 60); do
  IFS='|' read -r s h o <<<"$(snap "$TAB")"
  printf '\r  t=%ss state=%s pending=%s   ' "$((i/2))" "$s" "$h" >&2
  if [ "$h" = 1 ] && [ "$s" = question ]; then saw=1; val="$o"; echo; echo "  STRUCTURED QUESTION via SSE: state=question + pending_prompt present (option='$o')"; break; fi
  python3 -c 'import time;time.sleep(0.5)'
done
echo
if [ "$saw" != 1 ]; then
  echo "RESULT: NO-TRIGGER — opencode did not raise a structured permission (auto-approve config?). NOT an SSE-path proof. Ensure Option-A permission policy = ask."
  exit 2
fi

# NO-AUTO-SELECT observation window: a REAL blocking question must stay pending
# until WE respond. If it clears on its own here, the PRODUCT auto-selected/answered
# (not a true blocking permission) — that is a C9 failure, not a pass.
echo "== no-auto-select window: pending_prompt+state=question must stay STABLE 4s (no product auto-select) =="
for i in $(seq 1 8); do
  IFS='|' read -r s h o <<<"$(snap "$TAB")"
  printf '\r  +%0.1fs state=%s pending=%s   ' "$(python3 -c "print($i*0.5)")" "$s" "$h" >&2
  if [ "$h" != 1 ] || [ "$s" != question ]; then
    echo; echo "RESULT: AUTO-SELECTED — question cleared by the product BEFORE our respond (state=$s pending=$h). Not a real blocking question; not a C9 pass."
    exit 1
  fi
  python3 -c 'import time;time.sleep(0.5)'
done
echo; echo "  stable: question held for 4s with no auto-select"

echo "== structured respond (POST /prompt/respond value='$val') -> must CLEAR =="
post "/api/tabs/$TAB/prompt/respond" "$(python3 -c 'import json,sys;print(json.dumps({"value":sys.argv[1]}))' "${val:-yes}")" >/dev/null
cleared=0
for i in $(seq 1 30); do
  IFS='|' read -r s h o <<<"$(snap "$TAB")"
  printf '\r  t=%ss state=%s pending=%s   ' "$((i/2))" "$s" "$h" >&2
  if [ "$h" = 0 ] && [ "$s" != question ]; then cleared=1; echo; echo "  CLEARED: pending_prompt gone, state=$s"; break; fi
  python3 -c 'import time;time.sleep(0.5)'
done
echo
if [ "$saw" = 1 ] && [ "$cleared" = 1 ]; then
  echo "RESULT: PASS (C9 E2E — real SSE subscriber -> state=question -> structured respond cleared)"
  exit 0
fi
echo "RESULT: FAIL (question surfaced but structured respond did NOT clear it)"
exit 1
