#!/usr/bin/env bash
# Validate the centralized structured-prompt-server allocation fix.
#
# Root cause: allocate_server()/register_endpoint() are called ONLY from the
# orchestrator spawn path (service/orchestrator.rs ~516, gated on
# needs_structured_prompt_server). The direct/UI tab-creation path
# (multiplexer/commands.rs) hardcodes structured_prompt_server=None, so opencode
# spawned via UI/CLI gets NO --port -> no SSE/structured-prompt server -> no
# control plane and permission/questions fall back to grid parsing.
#
# This spawns opencode via the DIRECT/CLI path (the broken one) and asserts the
# server got allocated:
#   (1) launch command includes --port  (dev log build_agent_command)
#   (2) the opencode process opens a LISTEN socket on that port
#   (3) the server base is reachable (control plane present)
#
# BEFORE fix: all three FAIL (plain TUI). AFTER fix: all three PASS.
# Pure dev API + local inspection. Disposable tab only.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
source evals/_lib/lib.sh 2>/dev/null; export PATH="/usr/bin:/bin:$PATH"
LOG="$HOME/.codeterm-dev/logs/codeterm.log"
# dev_post/dev_get in the bench lib intermittently lose `curl` on PATH inside the
# function; use /usr/bin/curl directly with the dev token for robustness.
TOK=$(python3 -c "import re;print(re.search(r'token\s*=\s*\"([^\"]+)\"',open('$HOME/.codeterm-dev/config.toml').read()).group(1))")
ocpost(){ /usr/bin/curl -s -X POST -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' --data "$2" "http://127.0.0.1:7686$1"; }
ocget(){ /usr/bin/curl -s -H "Authorization: Bearer $TOK" "http://127.0.0.1:7686$1"; }

# --via selects the spawn path under test:
#   cli  (default) — `dc agent spawn` = the scriptable proxy for the NON-spawn_agent
#                    paths (desktop mux_spawn_tab / CLI). This is the path that needs
#                    the centralization fix. Currently launches WITHOUT --port.
#   rest           — POST /api/agents/spawn = svc.spawn_agent, which ALREADY allocates
#                    + subscribes. Green here is necessary but NOT sufficient for C8.
VIA="cli"
for a in "$@"; do case "$a" in --via=*) VIA="${a#--via=}";; --via) :;; cli|rest) VIA="$a";; esac; done

WS=$(dc workspace create "VIS-ocserver-check" --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
echo "workspace=$WS  spawn-path=$VIA"
trap 'dc workspace close "$WS" >/dev/null 2>&1 || true' EXIT
MARK=$(wc -l < "$LOG" 2>/dev/null || echo 0)
PID_BEFORE=$(pgrep -x opencode | sort)
if [ "$VIA" = rest ]; then
  tab=$(ocpost /api/agents/spawn "$(python3 -c 'import json,sys;print(json.dumps({"agent":"opencode","workspace_id":sys.argv[1],"activate":True}))' "$WS")" | python3 -c "import sys,json;print(json.load(sys.stdin).get('tab_id',''))" 2>/dev/null)
  echo "opencode tab=$tab (POST /api/agents/spawn = web-UI/REST path A, svc.spawn_agent)"
else
  tab=$(dc agent spawn opencode --workspace "$WS" --title ocserver --json 2>&1 | python3 -c "import sys,json;print(json.load(sys.stdin).get('tab_id',''))" 2>/dev/null)
  echo "opencode tab=$tab (dc agent spawn = CLI path C, proxy for non-spawn_agent paths)"
fi
python3 -c 'import time;time.sleep(9)'

# Isolate the NEW opencode process spawned by THIS check (PID diff). All checks
# read THIS pid's real state — NOT the shared dev log, which has no tab_id and is
# contaminated by concurrent spawns (the parallel-check artifact that produced a
# false PARTIAL: a concurrent REST spawn's --port line was mis-attributed to CLI).
NEWPIDS=$(comm -13 <(echo "$PID_BEFORE") <(pgrep -x opencode | sort))
echo "  new opencode pids since spawn: $(echo $NEWPIDS | xargs echo)"
OCPID=$(echo "$NEWPIDS" | head -1)

echo "== (1) launch command (THIS process argv, pid=$OCPID — contamination-proof) =="
CMD=$(ps -o command= -p "${OCPID:-0}" 2>/dev/null)
echo "  $CMD"
if echo "$CMD" | grep -q -- "--port"; then echo "  (1) PASS: --hostname/--port in THIS process argv"; P1=1; else echo "  (1) FAIL: no --port in THIS process argv"; P1=0; fi

echo "== (2) opencode LISTEN socket (same pid) =="
PORT=$(lsof -nP -p "${OCPID:-0}" 2>/dev/null | awk '/LISTEN/{print $9}' | sed 's/.*://' | head -1)
if [ -n "$PORT" ]; then echo "  (2) PASS: opencode pid=$OCPID listening on $PORT"; P2=1; else echo "  (2) FAIL: opencode pid=$OCPID has no LISTEN socket (plain TUI)"; P2=0; fi

echo "== (3) control-plane reachable =="
if [ -n "${PORT:-}" ] && curl -s --max-time 3 "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then
  echo "  (3) PASS: http://127.0.0.1:$PORT reachable"; P3=1
  echo "      base_url=http://127.0.0.1:$PORT session=$(ocget /api/tabs | python3 -c "import sys,json;d=json.load(sys.stdin);print(next((p.get('agent_session_id') for p in d if p['id']=='$tab'),'?'))")"
else echo "  (3) FAIL: no reachable control plane"; P3=0; fi

# C2 — server allocation WITHOUT a subscriber is not enough. maybe_start_subscriber()
# (orchestrator.rs:559) opens an SSE stream daemon->opencode:PORT/event. The external
# signal is an ESTABLISHED conn from the CodeTerm daemon (owns :7686) to that port.
echo "== (4) C2: SSE subscriber connected (daemon -> opencode:port ESTABLISHED) =="
DPID=$(lsof -nP -iTCP:7686 -sTCP:LISTEN 2>/dev/null | awk 'NR>1{print $2}' | head -1)
if [ -n "${PORT:-}" ] && [ -n "$DPID" ] && lsof -nP -p "$DPID" 2>/dev/null | grep -E "ESTABLISHED" | grep -q ":$PORT"; then
  echo "  (4) PASS: daemon pid=$DPID has ESTABLISHED SSE conn to 127.0.0.1:$PORT"; P4=1
else
  echo "  (4) FAIL: no daemon->opencode:$PORT SSE connection (server up but NO subscriber = C2 gap)"; P4=0
fi

# C8 / B-ORDER: the subscriber (P4) is only TRUSTED if the provider was actually
# launched with --hostname/--port (P1) — a daemon→port SSE conn without a confirmed
# launch could be a stale/wrong-port connection, not a real structured channel for
# THIS tab. Proof order is launch(P1) -> listen(P2) -> reachable(P3) -> subscribe(P4).
echo "== RESULT (path=$VIA) =="
if [ "${P4:-0}" = 1 ] && [ "${P1:-0}" != 1 ]; then
  echo "B-ORDER VIOLATION: subscriber-PASS but launch had NO --hostname/--port (P1 FAIL) — SSE conn not trusted for this tab"; exit 1
fi
if [ "${P1:-0}" = 1 ] && [ "${P2:-0}" = 1 ] && [ "${P3:-0}" = 1 ] && [ "${P4:-0}" = 1 ]; then
  echo "PASS [$VIA]: launched with --port (C8) AND server reachable AND subscriber active (C2) — out-of-the-box on this path"; exit 0
elif [ "${P1:-0}" = 1 ] && [ "${P4:-0}" != 1 ]; then
  echo "PARTIAL [$VIA]: launched with --port but NO subscriber (C2 gap — allocation without subscriber)"; exit 1
fi
echo "FAIL [$VIA]: no --hostname/--port on launch — provider NOT given a structured-prompt server (C8 gap, the path that needs the fix)"; exit 1
