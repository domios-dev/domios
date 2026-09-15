#!/usr/bin/env bash
# Layer B — agent-driven chat round-trip harness (macOS-runnable, no UI).
#
# THE ORACLE for "did my message land": spawns a REAL provider agent on the DEV
# daemon, sends a message through the SAME path the chat composer uses
# (POST /api/tabs/{id}/send → send_text_when_ready, identical to the Tauri
# `chat_send_text` command), then polls the PARSED chat transcript
# (GET /api/tabs/{id}/chat) until the user echo AND a following assistant reply
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
# (question + ALL options) was extracted into the tab's pending_prompt.
#
# Usage:
#   ./run.sh                            # claude + codex (codex runs the picker case)
#   CR_PROVIDERS="codex cursor" ./run.sh # both picker-extraction providers
#   CR_PROVIDERS="cursor" ./run.sh      # one provider
#   CR_TIMEOUT=120 ./run.sh             # per-turn poll budget (s)
#   CR_KEEP=1 ./run.sh                  # leave tabs/workspace for inspection
set -uo pipefail

report_verdict() {
  local failures="$1" providers="$2"
  echo
  if [ "$failures" -eq 0 ]; then
    echo "[messaging-core] ALL PASS — delivery + messaging core proven for: $providers"
    return 0
  fi
  echo "[messaging-core] $failures assertion(s) FAILED" >&2
  return 1
}

if [ "${1:-}" = "--verdict-only" ]; then
  [[ "${2:-}" =~ ^[0-9]+$ ]] || {
    echo "usage: $0 --verdict-only <failure-count>" >&2
    exit 2
  }
  report_verdict "$2" "${CR_PROVIDERS:-claude codex}"
  exit $?
fi

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"   # auto dev_env + `dc` (locked to dev 7686)
PY="$(command -v python3 || command -v python)"
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

tab_status() {
  dev_get "/api/tabs/$1/status" \
    | $PY -c "import json,sys;print(json.load(sys.stdin).get('status',''))" 2>/dev/null
}

# Legacy detached send seam (send_text_when_ready). The chat composer's REAL
# seam is the ledger transaction below (`ledger_send`) — both stay covered.
chat_send() {
  local tab="$1" text="$2"
  $PY -c "import json,sys;print(json.dumps({'text':sys.argv[1],'enter':True}))" "$text" \
    | curl -s --max-time 30 -X POST -H "$(dev_auth_header)" \
        -H "Content-Type: application/json" --data @- \
        "http://127.0.0.1:7686/api/tabs/$tab/send" >/dev/null
}

# Ledger transaction seam: POST /chat-send == Tauri chat_send_text. Returns the
# pending ChatSendStatus (send_id + state) for exact correlation.
ledger_send() {
  local tab="$1" text="$2"
  $PY -c "import json,sys;print(json.dumps({'text':sys.argv[1]}))" "$text" \
    | curl -s --max-time 60 -X POST -H "$(dev_auth_header)" \
        -H "Content-Type: application/json" --data @- \
        "http://127.0.0.1:7686/api/tabs/$tab/chat-send"
}

# Raw-keystroke seam (prod-docs/API.md `raw`): byte-for-byte PTY writes for the
# command followed by its carriage return.
send_raw_keys() {
  local tab="$1" text="$2" base="http://127.0.0.1:${CODETERM_API_PORT:-7686}"
  # MSYS vars: Git Bash on Windows rewrites argv "/model" into a C:/... path.
  MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL="*" \
    $PY -c "import json,sys;print(json.dumps({'text':sys.argv[1],'raw':True}))" "$text" \
    | curl -s --max-time 30 -X POST -H "$(dev_auth_header)" \
        -H "Content-Type: application/json" --data @- \
        "$base/api/tabs/$tab/send" >/dev/null
  curl -s --max-time 30 -X POST -H "$(dev_auth_header)" \
      -H "Content-Type: application/json" --data '{"text":"\r","raw":true}' \
      "$base/api/tabs/$tab/send" >/dev/null
}

# GET /chat-send = authoritative ledger reconcile; each read also runs the
# closed-chat evidence scan server-side.
ledger_statuses() { dev_get "/api/tabs/$1/chat-send"; }

ledger_state_of() {
  local tab="$1" send_id="$2"
  ledger_statuses "$tab" | $PY -c "
import json,sys
rows=[s for s in (json.load(sys.stdin) or []) if s.get('send_id')==sys.argv[1]]
print(rows[0]['state'] if rows else 'absent')" "$send_id" 2>/dev/null
}

await_ledger_state() {
  local tab="$1" send_id="$2"
  dev_run "$PY" - "$tab" "$send_id" "$TIMEOUT" <<'PY'
import json, os, queue, sys, threading, time, urllib.request

tab_id, send_id, timeout_text = sys.argv[1:]
deadline = time.monotonic() + int(timeout_text)
headers = {"Authorization": f"Bearer {os.environ['CODETERM_API_TOKEN']}"}
states = queue.Queue()
ready = threading.Event()
failure = queue.Queue(maxsize=1)

def stream_events():
    request = urllib.request.Request(
        "http://127.0.0.1:7686/api/events/stream",
        headers={**headers, "Accept": "text/event-stream"},
    )
    try:
        with urllib.request.urlopen(request, timeout=int(timeout_text) + 10) as response:
            ready.set()
            for raw in response:
                line = raw.decode("utf-8", errors="replace").strip()
                if not line.startswith("data:"):
                    continue
                event = json.loads(line[5:].strip())
                data = event.get("data") or {}
                if (
                    event.get("event_type") == "chat-send-status"
                    and data.get("tab_id") == tab_id
                    and data.get("send_id") == send_id
                ):
                    states.put(data.get("state", "absent"))
    except Exception as error:
        if failure.empty():
            failure.put(error)
        ready.set()

threading.Thread(target=stream_events, daemon=True).start()
if not ready.wait(timeout=min(10, int(timeout_text))):
    print("event_stream_unavailable")
    raise SystemExit(4)
if not failure.empty():
    print(f"event_stream_failed:{failure.get()}")
    raise SystemExit(4)

request = urllib.request.Request(
    f"http://127.0.0.1:7686/api/tabs/{tab_id}/chat-send", headers=headers
)
with urllib.request.urlopen(request, timeout=30) as response:
    rows = json.load(response) or []
current = next((row.get("state") for row in rows if row.get("send_id") == send_id), "absent")

while True:
    if current == "confirmed":
        print(current)
        raise SystemExit(0)
    if current == "unknown":
        print(current)
        raise SystemExit(2)
    if current == "failed":
        print(current)
        raise SystemExit(3)
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        print(current)
        raise SystemExit(4)
    try:
        current = states.get(timeout=remaining)
    except queue.Empty:
        print(current)
        raise SystemExit(4)
PY
}

assert_confirmed_state() {
  local provider="$1" case_name="$2" state="$3"
  case "$state" in
    confirmed) pass "$provider $case_name: exact turn evidence confirmed delivery" ;;
    accepted) fail "$provider $case_name: deadline reached while delivery remained accepted" ;;
    unknown) fail "$provider $case_name: delivery remained genuinely unconfirmed" ;;
    failed) fail "$provider $case_name: delivery failed" ;;
    *) fail "$provider $case_name: unexpected ledger state=$state" ;;
  esac
}

count_user_turns() {
  local tab="$1" needle="$2"
  chat_snapshot "$tab" | $PY -c "
import json,sys
doc=json.load(sys.stdin) or {}
msgs=doc.get('messages',[])
print(sum(1 for m in msgs if m.get('type')=='user' and sys.argv[1] in (m.get('content') or '')))" "$needle" 2>/dev/null
}

# Parsed transcript snapshot (the SAME normalized ChatMsg shape the chat UI
# renders). Stateless read — no tailer dependency.
chat_snapshot() { dev_get "/api/tabs/$1/chat?since=0"; }

# Full mux snapshot — carries each tab's pending_prompt (the extracted
# interactive prompt the chat card renders). Read-only.
mux_state() { dev_get /api/mux/state; }

# The UI seam the web chat view hits: chat_start_core, which returns
# {pending, messages}. Unlike the transcript-only GET route, it does not fail
# before the session binds — it returns pending:true, exactly what the chat UI
# sees. This is the oracle for the "stuck pending / session: -" binding bug.
chat_raw_start() { dev_post "/api/tabs/$1/chat-raw/start" '{"subscriptionId":"rt-harness"}'; }

# Poll the transcript until the user echo of `text` appears AND an assistant
# message follows it. Echoes a JSON capture tuple on success; empty on timeout.
poll_roundtrip() {
  local tab="$1" text="$2" sent_ms="$3" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local cap
    cap="$(chat_snapshot "$tab" | $PY "$RT" match-roundtrip "$text" "$sent_ms")"
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
  # --non-user-facing: bench prompts (e.g. the /model picker case) are the
  # harness's to answer — they must never bell the user's Input-needed inbox.
  dc agent spawn "$provider" --workspace "$ws" --cwd "$WS_CWD" --title "$title" --non-user-facing 2>&1 \
    | grep -oiE "tab [a-f0-9]+" | awk '{print $2}' | head -1
}

wait_live() {
  local tab="$1"
  for _ in $(seq 1 40); do
    local st; st="$(tab_status "$tab")"
    [ -n "$st" ] && [ "$st" != "Dead" ] && { sleep 4; return 0; }
    sleep 1
  done
  return 1
}

tab_screen() {
  dev_get "/api/tabs/$1/text" \
    | $PY -c "import json,sys;d=json.load(sys.stdin);print(d.get('text','') if isinstance(d,dict) else '')" 2>/dev/null
}

# Accept startup gate prompts (folder trust, provider self-update) exactly like
# a user: trust IS accepted, an update IS installed (never skipped) and the
# session gets time to restart. Loops until no gate remains.
clear_startup_gates() {
  local tab="$1" provider="$2" rounds=0
  while [ $rounds -lt 20 ]; do
    rounds=$((rounds + 1))
    local gate
    gate="$(mux_state | $PY "$RT" startup-gate "$tab")"
    if [ -n "$gate" ]; then
      local kind value
      kind="$(echo "$gate" | $PY -c "import json,sys;print(json.load(sys.stdin)['kind'])")"
      value="$(echo "$gate" | $PY -c "import json,sys;print(json.load(sys.stdin)['value'])")"
      echo "[chat-roundtrip] $provider: startup gate '$kind' — responding '$value'"
      $PY -c "import json,sys;print(json.dumps({'value':sys.argv[1]}))" "$value" \
        | curl -s --max-time 15 -X POST -H "$(dev_auth_header)" \
            -H "Content-Type: application/json" --data @- \
            "http://127.0.0.1:7686/api/tabs/$tab/prompt/respond" >/dev/null
      # An accepted update restarts the provider session — wait for it to
      # come back before looking for the next gate.
      [ "$kind" = "update" ] && wait_live "$tab"
      sleep 3
      continue
    fi
    # Fallback: trust dialog rendered but not extracted — default option is
    # the affirmative one, Enter accepts it.
    if tab_screen "$tab" | grep -qiE "trust this folder|do you trust"; then
      echo "[chat-roundtrip] $provider: raw trust dialog on screen — pressing Enter"
      dc send --actions enter --tab "$tab" >/dev/null 2>&1
      sleep 3
      continue
    fi
    return 0
  done
  echo "[chat-roundtrip] $provider: startup gates did not clear after $rounds rounds" >&2
  return 1
}

# --- Case: elementary round-trip ------------------------------------
# Send one trivially-answerable prompt; the agent echoes a nonce back. Proves:
# (a) the user message lands in the transcript, (b) an assistant reply follows.
case_elementary() {
  local provider="$1" tab="$2"
  local nonce; nonce="RT$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: PONG-$nonce"
  echo "[chat-roundtrip] $provider: elementary — send + await echo & reply"
  chat_raw_start "$tab" >/dev/null
  local sent_ms; sent_ms="$(now_ms)"
  chat_send "$tab" "$prompt"
  local cap; cap="$(poll_roundtrip "$tab" "$prompt" "$sent_ms")"
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
  local provider="$1" tab="$2"
  echo "[chat-roundtrip] $provider: rapid multi-send (3 quick messages)"
  local base; base="RR$(now_ms)"
  chat_raw_start "$tab" >/dev/null
  chat_send "$tab" "rapid one $base"; sleep 1
  chat_send "$tab" "rapid two $base"; sleep 1
  chat_send "$tab" "rapid three $base"
  local deadline=$((SECONDS + TIMEOUT)) ok=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    ok="$(chat_snapshot "$tab" | $PY "$RT" rapid-check "$base")"
    [ "$ok" = "1" ] && break
    sleep 2
  done
  if [ "$ok" = "1" ]; then
    pass "$provider rapid: all 3 quick sends landed in transcript"
  else
    fail "$provider rapid: not all quick sends appeared within ${TIMEOUT}s"
  fi
}

# --- Case: send-order preservation (transcript ordering) ------------
# Send N distinct messages in order 1..N; assert their user turns appear in the
# transcript in that SAME relative order. The regression gate for the stable-
# ordering fix: codex floors timestamps to whole seconds and cursor emits none,
# so a timestamp-only sort shuffled same-second / timeless turns. The transcript
# now carries a monotonic source-order `seq` that pins order regardless.
case_ordering() {
  local provider="$1" tab="$2" n=5
  echo "[chat-roundtrip] $provider: ordering — send $n distinct msgs, assert send-order in transcript"
  chat_raw_start "$tab" >/dev/null
  local base; base="OR$(now_ms)"
  local i
  for i in $(seq 1 "$n"); do
    chat_send "$tab" "msg $i of $base reply with K$i"
    sleep 1
  done
  local deadline=$((SECONDS + TIMEOUT)) res="missing"
  while [ "$SECONDS" -lt "$deadline" ]; do
    res="$(chat_snapshot "$tab" | $PY "$RT" order-check "$base" "$n")"
    [ "$res" = "ok" ] && break
    # Keep polling while messages are still landing; a real ordering failure also
    # resolves only by deadline, so we report whatever the final state is.
    sleep 2
  done
  case "$res" in
    ok) pass "$provider ordering: all $n messages in send order" ;;
    reorder*) fail "$provider ordering: transcript out of send order ($res)" ;;
    collapse*) fail "$provider ordering: transcript message boundary collapse ($res)" ;;
    *) fail "$provider ordering: not all $n messages landed within ${TIMEOUT}s ($res)" ;;
  esac
}

# --- Case: session-binding gate (CE3) -------------------------------
# A fresh agent tab MUST bind its session and DELIVER the transcript through the
# UI seam (chat-raw/start) — never sit on pending:true forever. That perpetual
# pending IS the bug Andrey saw: "Polo session: -", chat empty while the TUI
# works. This case reproduces it and is the gate proving the binding+delivery fix.
case_binding() {
  local provider="$1" tab="$2"
  echo "[chat-roundtrip] $provider: session-binding — fresh tab binds + chat-raw/start delivers (not pending)"
  local nonce; nonce="BIND$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: BOUND-$nonce"
  # Informational: the UI seam typically reports pending until the session binds.
  local init; init="$(chat_raw_start "$tab" | $PY "$RT" binding-state "$prompt")"
  echo "    initial chat-raw/start state: $init"
  chat_send "$tab" "$prompt"
  local deadline=$((SECONDS + TIMEOUT)) state="pending"
  while [ "$SECONDS" -lt "$deadline" ]; do
    state="$(chat_raw_start "$tab" | $PY "$RT" binding-state "$prompt")"
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
# the prompt — question + ALL options — into the tab's pending_prompt (the SAME
# field the chat card reads). This gates the per-provider parse path: codex's
# numbered menu via the universal parser; cursor via its new plugin parsePrompt.
# A non-skip-permissions tab prompts before running a shell command.
case_picker() {
  local provider="$1" tab="$2"
  echo "[chat-roundtrip] $provider: picker-extraction — drive a menu + assert question/options extracted"
  chat_raw_start "$tab" >/dev/null
  # /model shows the model picker — a real interactive menu that ALWAYS renders.
  # (Approval menus are suppressed when the tab is auto-approve/skip-permissions,
  # which is CodeTerm's spawn default — so they're not a reliable picker trigger.)
  send_raw_keys "$tab" "/model"
  local deadline=$((SECONDS + TIMEOUT)) cap=""
  while [ "$SECONDS" -lt "$deadline" ]; do
    cap="$(mux_state | $PY "$RT" pending-prompt "$tab")"
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
  dc send --actions esc --tab "$tab" >/dev/null 2>&1 || true
  dev_post "/api/tabs/$tab/prompt/clear" '{}' >/dev/null 2>&1 || true
}

# --- Case: ledger transaction (POST /chat-send) ----------------------
# The Chat composer's REAL seam. Asserts the authoritative ledger reaches
# state=confirmed for the exact send_id, the user turn lands EXACTLY once, and no
# failed entry (the false-Retry class) is retained for the tab.
case_ledger() {
  local provider="$1" tab="$2"
  local nonce; nonce="LG$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: LEDGER-$nonce"
  echo "[chat-roundtrip] $provider: ledger — chat-send reaches confirmed, exactly one user turn, no failure retained"
  local resp send_id
  resp="$(ledger_send "$tab" "$prompt")"
  send_id="$(echo "$resp" | $PY -c "import json,sys;print(json.load(sys.stdin).get('send_id',''))" 2>/dev/null)"
  if [ -z "$send_id" ]; then fail "$provider ledger: chat-send rejected: $resp"; return 1; fi
  local final; final="$(await_ledger_state "$tab" "$send_id")"
  assert_confirmed_state "$provider" "ledger send_id=$send_id" "$final"
  local n; n="$(count_user_turns "$tab" "LEDGER-$nonce")"
  if [ "$n" = "1" ]; then
    pass "$provider ledger: exactly one user turn in transcript"
  else
    fail "$provider ledger: user-turn count=$n (want exactly 1 — payload must be written once)"
  fi
  local failures
  failures="$(ledger_statuses "$tab" | $PY -c "import json,sys;print(sum(1 for s in (json.load(sys.stdin) or []) if s.get('state')=='failed'))" 2>/dev/null)"
  if [ "$failures" = "0" ]; then
    pass "$provider ledger: no failed/Retry entries retained"
  else
    fail "$provider ledger: $failures failed entr(ies) retained after delivery"
  fi
}

# --- Case: detached ledger delivery (closed-Chat evidence) -----------
# Stops the live chat stream FIRST, then sends through the ledger: no delta
# sink exists while the turn persists, so only the append-window evidence scan
# (run by GET /chat-send) can classify it. Reattach => confirmed, no Retry.
case_ledger_detached() {
  local provider="$1" tab="$2"
  local nonce; nonce="LD$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: DETACH-$nonce"
  echo "[chat-roundtrip] $provider: ledger-detached — no live sink during delivery; reattach reconciles to confirmed"
  # Runs first on the fresh tab, before any chat subscription starts.
  # so no delta tailer/poller exists — detached by construction.
  local resp send_id
  resp="$(ledger_send "$tab" "$prompt")"
  send_id="$(echo "$resp" | $PY -c "import json,sys;print(json.load(sys.stdin).get('send_id',''))" 2>/dev/null)"
  if [ -z "$send_id" ]; then fail "$provider ledger-detached: chat-send rejected: $resp"; return 1; fi
  # Wait for the turn to persist via the STATELESS transcript read (spawns no
  # tailer), then reattach through the ledger read that runs the scan.
  local deadline=$((SECONDS + TIMEOUT)) n=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    n="$(count_user_turns "$tab" "DETACH-$nonce")"
    [ "${n:-0}" -ge 1 ] && break
    sleep 2
  done
  if [ "${n:-0}" -lt 1 ]; then
    fail "$provider ledger-detached: user turn never persisted within ${TIMEOUT}s"
    return 1
  fi
  pass "$provider ledger-detached: exact turn persisted while detached"
  local final; final="$(await_ledger_state "$tab" "$send_id")"
  assert_confirmed_state "$provider" "ledger-detached send_id=$send_id" "$final"
}

# --- Case: ledger send after agent restart (rebind exact-once) -------
# Restart the agent (fresh PTY under the same tab), then a ledger send must
# reach confirmed with the payload written exactly once into the NEW session flow.
case_ledger_restart() {
  local provider="$1" tab="$2"
  echo "[chat-roundtrip] $provider: ledger-restart — agent restart + rebind, then exact-once ledger send"
  dev_post "/api/tabs/$tab/agent/restart" '{}' >/dev/null 2>&1
  if ! wait_live "$tab"; then
    fail "$provider ledger-restart: agent did not come back after restart"
    return 1
  fi
  clear_startup_gates "$tab" "$provider" || true
  # No settle sleep: the chat-send delivery thread itself waits on boot,
  # PTY link, and routability signals before writing.
  local nonce; nonce="RS$(now_ms)"
  local prompt="Reply with exactly this token and nothing else: REBIND-$nonce"
  local resp send_id
  resp="$(ledger_send "$tab" "$prompt")"
  send_id="$(echo "$resp" | $PY -c "import json,sys;print(json.load(sys.stdin).get('send_id',''))" 2>/dev/null)"
  if [ -z "$send_id" ]; then fail "$provider ledger-restart: chat-send rejected: $resp"; return 1; fi
  local final; final="$(await_ledger_state "$tab" "$send_id")"
  assert_confirmed_state "$provider" "ledger-restart send_id=$send_id" "$final"
  local n; n="$(count_user_turns "$tab" "REBIND-$nonce")"
  if [ "$n" = "1" ]; then
    pass "$provider ledger-restart: exactly one user turn after rebind"
  else
    fail "$provider ledger-restart: user-turn count=$n (want exactly 1)"
  fi
}

# --- Case: cross-agent messaging (messaging core) --------------------
# Direct envelope delivery A->B via the messenger seam, then an AGENT-DRIVEN
# reply: B is asked (through the ledger) to run `codeterm send` back to A.
# Proves inter-agent messages arrive in BOTH transports: host envelope and a
# live agent's own CLI send.
case_crossping() {
  local from_provider="$1" from_tab="$2" to_provider="$3" to_tab="$4"
  local nonce; nonce="XP$(now_ms)"
  echo "[messaging-core] $from_provider->$to_provider: envelope ping + agent-driven pong"
  $PY -c "import json,sys;print(json.dumps({'text':sys.argv[1],'source_tab':sys.argv[2]}))" \
      "MSGCORE-PING-$nonce" "$from_tab" \
    | curl -s --max-time 30 -X POST -H "$(dev_auth_header)" \
        -H "Content-Type: application/json" --data @- \
        "http://127.0.0.1:7686/api/tabs/$to_tab/send" >/dev/null
  local deadline=$((SECONDS + TIMEOUT)) n=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    n="$(count_user_turns "$to_tab" "MSGCORE-PING-$nonce")"
    [ "${n:-0}" -ge 1 ] && break
    sleep 2
  done
  if [ "${n:-0}" -ge 1 ]; then
    pass "$from_provider->$to_provider: envelope ping landed in recipient transcript"
  else
    fail "$from_provider->$to_provider: envelope ping never appeared in recipient transcript"
    return 1
  fi
  local reply_cmd="codeterm send \"MSGCORE-PONG-$nonce\" --tab $from_tab"
  local resp send_id
  resp="$(ledger_send "$to_tab" "Run exactly this shell command now and do nothing else: $reply_cmd")"
  send_id="$(echo "$resp" | $PY -c "import json,sys;print(json.load(sys.stdin).get('send_id',''))" 2>/dev/null)"
  [ -n "$send_id" ] || { fail "$to_provider: pong instruction send rejected: $resp"; return 1; }
  deadline=$((SECONDS + TIMEOUT)); n=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    n="$(count_user_turns "$from_tab" "MSGCORE-PONG-$nonce")"
    [ "${n:-0}" -ge 1 ] && break
    sleep 2
  done
  if [ "${n:-0}" -ge 1 ]; then
    pass "$to_provider->$from_provider: agent-driven pong arrived back"
  else
    fail "$to_provider->$from_provider: agent-driven pong never arrived within ${TIMEOUT}s"
  fi
}

provider_of() { echo "$1" | cut -d: -f1; }
tab_of() { echo "$1" | cut -d: -f2; }

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
  echo "[messaging-core] workspace=$ws providers='$PROVIDERS' timeout=${TIMEOUT}s out=$OUT_DIR"

  # Phase 1 — spawn the WHOLE matrix simultaneously, then gate-clear each tab.
  local pairs=""
  for provider in $PROVIDERS; do
    local tab; tab="$(spawn_agent "$provider" "$ws" "rt-$provider")"
    if [ -z "$tab" ]; then fail "$provider: agent spawn failed"; continue; fi
    echo "[messaging-core] spawned $provider tab=$tab"
    pairs="$pairs $provider:$tab"
  done
  local live=""
  for pair in $pairs; do
    local provider tab
    provider="$(provider_of "$pair")"; tab="$(tab_of "$pair")"
    if ! wait_live "$tab"; then
      fail "$provider: agent never came live"
      [ "${CR_KEEP:-0}" = "1" ] || dc tab close "$tab" >/dev/null 2>&1
      continue
    fi
    clear_startup_gates "$tab" "$provider" || true
    live="$live $provider:$tab"
  done

  # Phase 2 — per-provider delivery matrix (ledger-detached FIRST: the fresh
  # tab has no chat stream yet, so the closed-Chat evidence journey is real).
  for pair in $live; do
    local provider tab
    provider="$(provider_of "$pair")"; tab="$(tab_of "$pair")"
    echo; echo "=== provider: $provider (tab=$tab) ==="
    case_ledger_detached "$provider" "$tab"
    case_binding "$provider" "$tab"
    case_elementary "$provider" "$tab"
    case_rapid "$provider" "$tab"
    case_ordering "$provider" "$tab"
    case_ledger "$provider" "$tab"
    # Picker-extraction gate (facet9 P9.3/P9.4): /model populates pending_prompt.
    case "$provider" in claude|cursor|codex) case_picker "$provider" "$tab" ;; esac
  done

  # Phase 3 — cross-agent messaging ring: each live agent pings the next.
  set -- $live
  local count=$#
  if [ "$count" -ge 2 ]; then
    echo; echo "=== messaging core: cross-agent ring ($count agents) ==="
    local i=1
    for pair in $live; do
      local next_index=$((i % count + 1))
      local next_pair; next_pair="$(eval echo \${$next_index})"
      case_crossping "$(provider_of "$pair")" "$(tab_of "$pair")" \
                     "$(provider_of "$next_pair")" "$(tab_of "$next_pair")"
      i=$((i + 1))
    done
  fi

  # Phase 4 — restart survival: restart every agent, then exact-once ledger
  # send and a post-restart ring pass prove delivery survives rebind.
  echo; echo "=== messaging core: restart survival ==="
  for pair in $live; do
    case_ledger_restart "$(provider_of "$pair")" "$(tab_of "$pair")"
  done
  if [ "$count" -ge 2 ]; then
    echo; echo "=== messaging core: post-restart ring ==="
    local i=1
    for pair in $live; do
      local next_index=$((i % count + 1))
      local next_pair; next_pair="$(eval echo \${$next_index})"
      case_crossping "$(provider_of "$pair")" "$(tab_of "$pair")" \
                     "$(provider_of "$next_pair")" "$(tab_of "$next_pair")"
      i=$((i + 1))
    done
  fi

  for pair in $live; do
    [ "${CR_KEEP:-0}" = "1" ] || dc tab close "$(tab_of "$pair")" >/dev/null 2>&1
  done

  report_verdict "$FAILS" "$PROVIDERS"
}

main "$@"
