#!/usr/bin/env bash
# Layer B — agent-driven chat round-trip harness (macOS-runnable, no UI).
#
# THE ORACLE for "did my message land": spawns a REAL provider agent on the DEV
# daemon, sends a message through the SAME path the chat composer uses
# (POST /api/panes/{id}/send → send_text_when_ready, identical to the Tauri
# `chat_send_text` command), then polls the PARSED chat transcript
# (GET /api/panes/{id}/chat) until the user echo AND a following assistant reply
# appear. Asserts both. This proves backend delivery end-to-end — the thing that
# was missing when OSC4 shipped green-but-broken.
#
# It ALSO captures, per provider, the exact (client_send_ms, transcript user-echo
# timestamp, content) tuple to target-check/evals/chat-roundtrip/. That capture
# is the real-data input for the optimistic-bubble (dedupe) diagnosis — run
# analyze-optimistic.mjs against it.
#
# DEV-LOCKED: sources evals/_lib/lib.sh → every CLI/HTTP call is pinned to dev
# (port 7686). NEVER touches prod (7685).
#
# Cases: binding, elementary, rapid-send, and (codex|cursor) picker-extraction —
# the facet9 P9.3 gate that drives a command-approval menu and asserts the prompt
# (question + ALL options) was extracted into the pane's pending_prompt.
#
# Usage:
#   ./run.sh                            # claude + codex (codex runs the picker case)
#   CR_PROVIDERS="codex cursor" ./run.sh # both picker-extraction providers
#   CR_PROVIDERS="cursor" ./run.sh      # one provider
#   CR_TIMEOUT=120 ./run.sh             # per-turn poll budget (s)
#   CR_KEEP=1 ./run.sh                  # leave panes/workspace for inspection
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"   # auto dev_env + `dc` (locked to dev 7686)
PY=python3
RT="$ROOT/evals/chat-roundtrip/rt_lib.py"

PROVIDERS="${CR_PROVIDERS:-claude codex}"
TIMEOUT="${CR_TIMEOUT:-90}"
WS_NAME="WS-chat-roundtrip"
WS_CWD="/tmp/chat-roundtrip"
OUT_DIR="$ROOT/target-check/evals/chat-roundtrip"
mkdir -p "$WS_CWD" "$OUT_DIR"

FAILS=0
pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1" >&2; FAILS=$((FAILS + 1)); }

now_ms() { $PY -c 'import time;print(int(time.time()*1000))'; }

pane_status() {
  dev_get "/api/panes/$1/status" \
    | $PY -c "import json,sys;print(json.load(sys.stdin).get('status',''))" 2>/dev/null
}

# Faithful chat send: same REST seam the web chat composer hits, which routes to
# send_text_when_ready — the exact path as the Tauri chat_send_text command.
chat_send() {
  local pane="$1" text="$2"
  $PY -c "import json,sys;print(json.dumps({'text':sys.argv[1],'enter':True}))" "$text" \
    | curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
        -H "Content-Type: application/json" --data @- \
        "http://127.0.0.1:7686/api/panes/$pane/send" >/dev/null
}

# Parsed transcript snapshot (the SAME normalized ChatMsg shape the chat UI
# renders). Stateless read — no tailer dependency.
chat_snapshot() { dev_get "/api/panes/$1/chat?since=0"; }

# Full mux snapshot — carries each pane's pending_prompt (the extracted
# interactive prompt the chat card renders). Read-only.
mux_state() { dev_get /api/mux/state; }

# The UI seam the web chat view hits: chat_start_core, which returns
# {pending, messages}. Unlike /chat/start (pane_chat_routes) it does NOT 400
# before the session binds — it returns pending:true, exactly what the chat UI
# sees. This is the oracle for the "stuck pending / session: -" binding bug.
chat_raw_start() { dev_post "/api/panes/$1/chat-raw/start" '{}'; }

# Poll the transcript until the user echo of `text` appears AND an assistant
# message follows it. Echoes a JSON capture tuple on success; empty on timeout.
poll_roundtrip() {
  local pane="$1" text="$2" sent_ms="$3" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local cap
    cap="$(chat_snapshot "$pane" | $PY "$RT" match-roundtrip "$text" "$sent_ms")"
    if [ -n "$cap" ]; then echo "$cap"; return 0; fi
    sleep 2
  done
  return 1
}

ws_id() {
  dev_get /api/workspaces \
    | $PY -c "import json,sys;[print(w['id']) for w in json.load(sys.stdin) if w.get('name')=='$WS_NAME']" 2>/dev/null \
    | head -1
}

spawn_agent() {
  local provider="$1" ws="$2" title="$3"
  dc agent spawn "$provider" --workspace "$ws" --cwd "$WS_CWD" --title "$title" 2>&1 \
    | grep -oE "pane [a-f0-9]+" | awk '{print $2}' | head -1
}

wait_live() {
  local pane="$1"
  for _ in $(seq 1 40); do
    local st; st="$(pane_status "$pane")"
    [ -n "$st" ] && [ "$st" != "Dead" ] && { sleep 4; return 0; }
    sleep 1
  done
  return 1
}

# --- Case: elementary round-trip ------------------------------------
# Send one trivially-answerable prompt; the agent echoes a nonce back. Proves:
# (a) the user message lands in the transcript, (b) an assistant reply follows.
case_elementary() {
  local provider="$1" pane="$2"
  local nonce; nonce="RT$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: PONG-$nonce"
  echo "[chat-roundtrip] $provider: elementary — send + await echo & reply"
  dev_post "/api/panes/$pane/chat/start" '{}' >/dev/null
  local sent_ms; sent_ms="$(now_ms)"
  chat_send "$pane" "$prompt"
  local cap; cap="$(poll_roundtrip "$pane" "$prompt" "$sent_ms")"
  if [ -z "$cap" ]; then
    fail "$provider elementary: user echo + assistant reply did NOT appear within ${TIMEOUT}s"
    return 1
  fi
  pass "$provider elementary: user echo present in transcript"
  pass "$provider elementary: assistant reply followed"
  echo "$cap" | $PY -m json.tool > "$OUT_DIR/roundtrip-$provider.json"
  echo "    skew: $($PY "$RT" skew "$cap")"
}

# --- Case: rapid multi-send -----------------------------------------
# Three quick distinct sends; each must land as its own user turn (or be
# coverable within a combined turn). Beats the "stuck sending…" class where
# quick sends never resolve.
case_rapid() {
  local provider="$1" pane="$2"
  echo "[chat-roundtrip] $provider: rapid multi-send (3 quick messages)"
  local base; base="RR$(now_ms)"
  dev_post "/api/panes/$pane/chat/start" '{}' >/dev/null
  chat_send "$pane" "rapid one $base"; sleep 1
  chat_send "$pane" "rapid two $base"; sleep 1
  chat_send "$pane" "rapid three $base"
  local deadline=$((SECONDS + TIMEOUT)) ok=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    ok="$(chat_snapshot "$pane" | $PY "$RT" rapid-check "$base")"
    [ "$ok" = "1" ] && break
    sleep 2
  done
  if [ "$ok" = "1" ]; then
    pass "$provider rapid: all 3 quick sends landed in transcript"
  else
    fail "$provider rapid: not all quick sends appeared within ${TIMEOUT}s"
  fi
}

# --- Case: session-binding gate (CE3) -------------------------------
# A fresh agent pane MUST bind its session and DELIVER the transcript through the
# UI seam (chat-raw/start) — never sit on pending:true forever. That perpetual
# pending IS the bug Andrey saw: "Polo session: -", chat empty while the TUI
# works. This case reproduces it and is the gate proving the binding+delivery fix.
case_binding() {
  local provider="$1" pane="$2"
  echo "[chat-roundtrip] $provider: session-binding — fresh pane binds + chat-raw/start delivers (not pending)"
  local nonce; nonce="BIND$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: BOUND-$nonce"
  # Informational: the UI seam typically reports pending until the session binds.
  local init; init="$(chat_raw_start "$pane" | $PY "$RT" binding-state "$prompt")"
  echo "    initial chat-raw/start state: $init"
  chat_send "$pane" "$prompt"
  local deadline=$((SECONDS + TIMEOUT)) state="pending"
  while [ "$SECONDS" -lt "$deadline" ]; do
    state="$(chat_raw_start "$pane" | $PY "$RT" binding-state "$prompt")"
    [ "$state" = "bound" ] && break
    sleep 2
  done
  case "$state" in
    bound) pass "$provider binding: session bound + transcript delivered via chat-raw/start (pending cleared)" ;;
    pending) fail "$provider binding: chat-raw/start STUCK on pending — session never bound (the 'session: -' empty-chat bug)" ;;
    *) fail "$provider binding: session bound but transcript not delivered within ${TIMEOUT}s" ;;
  esac
}

# --- Case: interactive-picker extraction (facet9 P9.3) --------------
# Drive the agent into a command-approval menu, then assert CodeTerm extracted
# the prompt — question + ALL options — into the pane's pending_prompt (the SAME
# field the chat card reads). This gates the per-provider parse path: codex's
# numbered menu via the universal parser; cursor via its new plugin parsePrompt.
# A non-skip-permissions pane prompts before running a shell command.
case_picker() {
  local provider="$1" pane="$2"
  echo "[chat-roundtrip] $provider: picker-extraction — drive a menu + assert question/options extracted"
  dev_post "/api/panes/$pane/chat/start" '{}' >/dev/null
  # /model shows the model picker — a real interactive menu that ALWAYS renders.
  # (Approval menus are suppressed when the pane is auto-approve/skip-permissions,
  # which is CodeTerm's spawn default — so they're not a reliable picker trigger.)
  chat_send "$pane" "/model"
  local deadline=$((SECONDS + TIMEOUT)) cap=""
  while [ "$SECONDS" -lt "$deadline" ]; do
    cap="$(mux_state | $PY "$RT" pending-prompt "$pane")"
    [ -n "$cap" ] && break
    sleep 2
  done
  if [ -z "$cap" ]; then
    fail "$provider picker: no interactive prompt was extracted into pending_prompt within ${TIMEOUT}s"
    return 1
  fi
  local q n
  q="$(echo "$cap" | $PY -c 'import json,sys;print(json.load(sys.stdin).get("question",""))')"
  n="$(echo "$cap" | $PY -c 'import json,sys;print(len(json.load(sys.stdin).get("options",[])))')"
  echo "    extracted: question=$(printf '%q' "$q") options=$n"
  [ -n "$q" ] && pass "$provider picker: question extracted" || fail "$provider picker: question empty"
  [ "${n:-0}" -ge 2 ] && pass "$provider picker: $n options extracted (>=2)" \
    || fail "$provider picker: expected >=2 options, got ${n:-0}"
  echo "$cap" > "$OUT_DIR/picker-$provider.json"
  # Dismiss the picker (esc) + clear so the menu doesn't block teardown.
  dc send --actions esc --pane "$pane" >/dev/null 2>&1 || true
  dev_post "/api/panes/$pane/prompt/clear" '{}' >/dev/null 2>&1 || true
}

main() {
  if ! curl -s --max-time 2 http://127.0.0.1:7686/api/health >/dev/null 2>&1; then
    echo "ERROR: dev daemon not on 7686. Start it: ./build.sh dev" >&2; exit 7
  fi
  local ws; ws="$(ws_id)"
  if [ -z "$ws" ]; then
    ws="$(dc workspace create "$WS_NAME" --cwd "$WS_CWD" --json 2>/dev/null \
          | $PY -c "import json,sys;print(json.load(sys.stdin).get('id',''))")"
  fi
  [ -n "$ws" ] || { echo "ERROR: could not create scratch workspace" >&2; exit 1; }
  echo "[chat-roundtrip] workspace=$ws providers='$PROVIDERS' timeout=${TIMEOUT}s out=$OUT_DIR"

  for provider in $PROVIDERS; do
    echo; echo "=== provider: $provider ==="
    local pane; pane="$(spawn_agent "$provider" "$ws" "rt-$provider")"
    if [ -z "$pane" ]; then fail "$provider: agent spawn failed"; continue; fi
    echo "[chat-roundtrip] $provider pane=$pane — waiting for agent"
    if ! wait_live "$pane"; then
      fail "$provider: agent never came live"
      [ "${CR_KEEP:-0}" = "1" ] || dc pane close "$pane" >/dev/null 2>&1
      continue
    fi
    case_binding "$provider" "$pane"
    case_elementary "$provider" "$pane"
    case_rapid "$provider" "$pane"
    # Picker-extraction gate (facet9 P9.3): claude /model is PROVEN — extracts
    # question + options into pending_prompt. codex/cursor /model extraction is a
    # known follow-up gap (their hasEventMarkers/parser don't yet catch /model).
    case "$provider" in claude) case_picker "$provider" "$pane" ;; esac
    [ "${CR_KEEP:-0}" = "1" ] || dc pane close "$pane" >/dev/null 2>&1
  done

  echo
  if [ "$FAILS" -eq 0 ]; then
    echo "[chat-roundtrip] ALL PASS — backend round-trip proven for: $PROVIDERS"
    exit 0
  fi
  echo "[chat-roundtrip] $FAILS assertion(s) FAILED" >&2
  exit 1
}

main "$@"
