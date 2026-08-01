#!/usr/bin/env bash
# [NON-GATING — prompt/model dependent] Structured-prompt elicitation SMOKE.
#
# DO NOT use as a C9 acceptance gate. Measured 2026-06-21: opencode (deepseek-v4-
# flash-free, auto-approve) renders polls as PROSE and self-answers, and executes
# tools WITHOUT a permission prompt — so NO question.asked/permission.asked event is
# emitted and the tab never reaches state=question. That is the MODEL not asking,
# NOT an SSE-path failure. The deterministic C9 gate is a local SSE fixture that
# drives the real subscriber -> pending_prompt -> state=Question -> respond roundtrip
# (Lead-owned). This script remains only as a supplemental LIVE elicitation smoke; a
# real opencode permission-policy trigger could make it deterministic later.
#
# Original intent (kept): ask opencode to raise a question/poll, watch for
# state==question (which can only come from the structured picker, never the
# visible-cursor grid).
#
# Why state==question is a sufficient witness: the live classifier
# (offline_state_classifier::classify_state) only yields Question from a STRUCTURED
# picker (step 1, picker_question) — a raw grid menu requires cursor_hidden, which
# opencode's visible-cursor inline composer is NOT. So a Question verdict here can
# ONLY have come through the SSE subscriber (C2). If it instead settles idle/working,
# the structured path did not deliver (server-without-subscriber, or no server).
#
# Pure dev API. Disposable tab (spawn one, or pass --tab). Cleans up if it spawned.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
source evals/_lib/lib.sh 2>/dev/null; export PATH="/usr/bin:/bin:$PATH"
TOK=$(python3 -c "import re;print(re.search(r'token\s*=\s*\"([^\"]+)\"',open('$HOME/.codeterm-dev/config.toml').read()).group(1))")
ocpost(){ /usr/bin/curl -s -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' --data "$2" "http://127.0.0.1:7686$1"; }
ocget(){ /usr/bin/curl -s -H "Authorization: Bearer $TOK" "http://127.0.0.1:7686$1"; }

TAB=""; SPAWNED_WS=""
PROMPT="Ask me a multiple-choice question with options A, B, C using your real interactive question/poll UI (not prose). Wait for my choice."
while [ "$#" -gt 0 ]; do case "$1" in --tab) TAB="$2"; shift 2;; --prompt) PROMPT="$2"; shift 2;; *) echo "unknown $1"; exit 2;; esac; done

st(){ ocget /api/tabs 2>/dev/null | python3 -c "import sys,json;d=json.load(sys.stdin);print(next(((p.get('agent_state') or {}).get('state') for p in d if p['id']=='$1'),'?'))"; }

if [ -z "$TAB" ]; then
  SPAWNED_WS=$(dc workspace create "VIS-structprompt" --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
  trap 'dc workspace close "$SPAWNED_WS" >/dev/null 2>&1 || true' EXIT
  TAB=$(dc agent spawn opencode --workspace "$SPAWNED_WS" --title "structprompt" --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin).get('tab_id',''))")
  echo "spawned opencode tab=$TAB (ws=$SPAWNED_WS)"
  python3 -c 'import time;time.sleep(9)'
fi

# Establish an IDLE baseline first — clear any startup splash/question so a later
# Question can ONLY be a response to our prompt (not the startup artifact that made
# a t=0s 'question' read falsely as success).
echo "== settling to idle baseline (NO auto-Enter — never press keys during baseline) =="
for i in $(seq 1 40); do
  s=$(st "$TAB")
  [ "$s" = question ] && { echo "  baseline has a pending question — refusing to auto-answer (would mask product behavior)"; break; }
  [ "$s" = idle ] && break
  python3 -c 'import time;time.sleep(0.5)'
done
base=$(st "$TAB"); echo "  baseline state=$base"
[ "$base" = idle ] || { echo "RESULT: INCONCLUSIVE (never settled to idle baseline, state=$base) — cannot attribute a later Question to the prompt"; exit 2; }

echo "== sending question-eliciting prompt =="
ocpost "/api/tabs/$TAB/send" "$(python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"enter":True}))' "$PROMPT")" >/dev/null

echo "== watching for idle->Question transition caused by the prompt (up to 40s) =="
saw_q=0
for i in $(seq 1 80); do
  s=$(st "$TAB")
  printf '\r  t=%ss state=%s    ' "$((i/2))" "$s" >&2
  [ "$s" = question ] && { saw_q=1; echo; echo "  reached state=question @ ~$((i/2))s after prompt"; break; }
  python3 -c 'import time;time.sleep(0.5)'
done
echo

if [ "$saw_q" = 1 ]; then
  echo "RESULT: STRUCTURED PROMPT DELIVERED (state=question via SSE picker — C2 path live)"
  exit 0
else
  echo "RESULT: NO STRUCTURED QUESTION (final state=$(st "$TAB")) — SSE path did not deliver (server/subscriber gap or opencode answered in prose)"
  exit 1
fi
