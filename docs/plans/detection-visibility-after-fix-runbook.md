# After-fix validation runbook — OpenCode composer + interrupt

Standing, deterministic checklist to run the moment Lead lands the OpenCode fixes
(composer_ready for the non-alt-screen inline composer, and a working interrupt path).
Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5).
Before-baseline captured at dev/classifier build state of 2026-06-21 ~19:21 (R9 WIP).

All numbers are produced by the SAME production classifier the live path uses
(`render_timeline.rs` → `offline_state_classifier`). The python readers consume the
BAKED `detector` field, so **Step 0 (regen) is mandatory** after any Rust change.

## Step 0 — rebuild + regenerate (mandatory after any classifier edit)
```bash
cd src-tauri && cargo build --example render_timeline && cd ..
REGEN=<scratch>/regen-afterfix; rm -rf "$REGEN"; mkdir -p "$REGEN"
RT=src-tauri/target/debug/examples/render_timeline
for c in $(find testbench/replays -name raw.cast); do d=$(dirname "$c");
  "$RT" "$c" "$REGEN/$(echo "$d"|sed 's#testbench/replays/##;s#/#__#g').json" \
    $([ -f "$d/meta.json" ]&&echo --meta "$d/meta.json") \
    $([ -f "$d/phases.json" ]&&echo --phases "$d/phases.json"); done
```
Verify build identity first: `curl -s :7686/api/health | python3 -c 'import sys,json;print(json.load(sys.stdin)["build"])'`
must equal `git rev-parse --short HEAD` (else the LIVE checks below run a stale daemon).

## Step 1 — transition-latency gate
```bash
python3 testbench/transition-latency.py "$REGEN" --gate
```
| provider | BEFORE (R9) safety-net | PASS threshold (after) |
|---|---|---|
| claude | 0/86 | **0** (unchanged) |
| codex | 0/55 | **0** (unchanged) |
| cursor | 0/44 | **0** (unchanged) |
| opencode | **4/19** | **0/N** — every opencode working→idle via composer anchor, sem_med < 200ms |

PASS = `GATE: GREEN` (all providers 0 safety-net). Regression guard: claude/codex/cursor
must NOT acquire any new safety-net transitions.

## Step 2 — strobe metric
```bash
python3 testbench/strobe-metric.py "$REGEN" --glob 'opencode__*.json' --bundle <scratch>/after-bundle
python3 testbench/strobe-metric.py "$REGEN" --glob '*.json'           # full sweep
```
| scope | BEFORE strobes | BEFORE ambiguous frames | PASS threshold (after) |
|---|---|---|---|
| opencode (all) | 44 | 1423 | strobes **0**; idle-matrix 737 ambiguous → resolve to 1 Idle run |
| opencode idle-matrix | 2 | 737 | ambiguous (working,alt=false) count → **0** (all become idle) |
| opencode systems_design | 41 | 660 | strobes **0** |
| full corpus | 156 | — | opencode contribution → 0; claude/codex/cursor must not increase |

PASS = opencode strobes 0 AND the 737 structurally-identical idle-matrix frames no longer
split working/idle. (claude 87 / codex 16 / cursor 9 are a separate, lower-priority
inspection item — only guard against INCREASE here.)

## Step 3 — non-vacuous /stop oracle (LIVE, disposable dev panes)
```bash
# create disposable ws, spawn agent, clear startup prompt, then:
testbench/stop-oracle.sh --pane <disposable_opencode> --mode stop   # repeat x3 (reliability)
testbench/stop-oracle.sh --pane <disposable_opencode> --mode esc
# regression guard:
testbench/stop-oracle.sh --pane <disposable_claude> --mode stop
testbench/stop-oracle.sh --pane <disposable_codex>  --mode stop
```
Only count NON-VACUOUS runs (pre-stop baseline `changing=YES`; the tool prints `VACUOUS`
and exits 2 otherwise — rerun with a longer prompt).

| provider/mode | BEFORE | PASS threshold (after) |
|---|---|---|
| opencode /stop | NOT INTERRUPTED (+108KB, reverted to working) in 2/2 non-vacuous | **TRULY HALTED** in ≥3/3 non-vacuous (raw growth <300B over 2000–6000ms post-stop, state idle) |
| opencode manual Esc | HALTED but STATE LAGGED ~5s | generation halts AND state idle < ~1.5s |
| claude /stop | TRULY HALTED | TRULY HALTED (no regression) |
| codex /stop | TRULY HALTED | TRULY HALTED (no regression) |

Disposable-pane discipline: create dev workspace via `dc workspace create`, `dc agent
spawn`, run, then `dc workspace close <id>`. Never target prod or Andrey's/Lead's panes.

## Step 4 — Codex ordering (unchanged scope)
```bash
python3 testbench/chat-order-gate.py testbench/fixtures/codex-chat-order-2bace0d6.json
python3 testbench/chat-order-gate.py --live --agent codex
```
PASS = GREEN (data-layer ordered). DOM render order remains MANUAL / Windows-only — not
assertable from macOS; the optimistic-merge render path needs a jsdom unit or Windows
Playwright DOM check (frontend item, out of macOS-API scope).

## rev15 acceptance-suite wiring (exact commands)
Single after-fix pass (regen → gate → strobe → permission → ordering). Run from repo root:
```bash
# 0. identity + regen against current classifier
(cd src-tauri && cargo build --example render_timeline)
REGEN=<scratch>/regen; rm -rf "$REGEN"; mkdir -p "$REGEN"
for c in $(find testbench/replays -name raw.cast); do d=$(dirname "$c");
  src-tauri/target/debug/examples/render_timeline "$c" \
    "$REGEN/$(echo "$d"|sed 's#testbench/replays/##;s#/#__#g').json" \
    $([ -f "$d/meta.json" ]&&echo --meta "$d/meta.json") $([ -f "$d/phases.json" ]&&echo --phases "$d/phases.json"); done
# 1. latency gate  (PASS = 0 safety-net all providers)
python3 testbench/transition-latency.py "$REGEN" --gate
# 2. strobe metric (PASS = 0 strobes; opencode question<->working is the open item)
python3 testbench/strobe-metric.py "$REGEN" --glob '*.json'
# 3. permission false-idle guard (PASS = not idle on the choice box)
testbench/permission-guard-check.sh
# 4. Codex transcript ordering (data layer)
python3 testbench/chat-order-gate.py testbench/fixtures/codex-chat-order-2bace0d6.json
# 5. LIVE /stop (disposable dev panes; opencode pending server-mode)
testbench/stop-oracle.sh --pane <disposable> --mode stop
```

## OpenCode control-plane (HTTP abort) — discovery result
- Current opencode launch = plain TUI: `opencode -m <model>` (dev log
  `orchestrator build_agent_command`), **no `--port`, no listening socket** (only an
  outbound HTTPS to the model API). **No HTTP control plane exists to abort.**
- CodeTerm reads opencode chat from its **on-disk SQLite DB** via
  `read_structured_chat` (`host.fs.sqliteQuery`) + a per-pane `structured_poller`; the
  installed opencode binding plugin PUSHES lifecycle hooks to CodeTerm's API. Direction
  is opencode→CodeTerm, so CodeTerm holds NO opencode base URL.
- Session id is known: `ses_…` (dev log `structured-poller … sid=`, and `agent_session_id`).
- For HTTP abort, Lead must launch opencode in **server mode**: `opencode serve
  --hostname 127.0.0.1 --port <P>` (or wire the `structuredPromptServer`/`sps.port` path
  that appends `--hostname/--port`). Control plane then = `http://127.0.0.1:<P>`, abort =
  `POST /session/{id}/abort`. Validate with:
  `testbench/stop-oracle.sh --pane <disposable> --mode http-abort --oc-base http://127.0.0.1:<P> --oc-session ses_…`
- Until server-mode/abort exists, **opencode /stop is UNRESOLVED** (raw ESC does not halt
  active generation — r6: +108KB after stop, reverted to working).

## Centralized structured-prompt-server allocation — validation
Root cause: `allocate_server()` / `register_endpoint()` (service/structured_prompt/mod.rs)
are called only from the orchestrator spawn path (orchestrator.rs ~516, gated on
`needs_structured_prompt_server`). The direct/UI path (multiplexer/commands.rs) hardcodes
`structured_prompt_server: None`. opencode manifest has `supportsStructuredPrompts:true,
structuredPromptServer:true`, so it SHOULD get a server on every spawn — it doesn't on the
direct/CLI path.

`testbench/opencode-server-check.sh` — spawns opencode via the DIRECT/CLI path and asserts:
1. launch command includes `--port` (dev-log `build_agent_command`)
2. the opencode process opens a LISTEN socket on that port
3. the control plane is reachable (`curl http://127.0.0.1:<port>/`)
4. **C2 — SSE subscriber connected**: the CodeTerm daemon (owns :7686) holds an
   ESTABLISHED connection to `127.0.0.1:<port>` (the `/event` SSE stream from
   `maybe_start_subscriber`, orchestrator.rs:559 — ALSO orchestrator-path-only today).

**Path selection + C8/B-ORDER** (`--via cli|rest`): the REST path (`/api/agents/spawn`
→ `svc.spawn_agent`) ALREADY allocates+subscribes (committed) — `--via rest` PASSes today
and is necessary-but-NOT-sufficient for out-of-the-box. The gap is the non-`spawn_agent`
paths (desktop `mux_spawn_pane`→`spawn_pane_atomic`, and CLI). `--via cli` (`dc agent
spawn`) is the scriptable proxy for them; it FAILs today (no `--hostname/--port`).
B-ORDER (C8): the subscriber (4) is only TRUSTED if the launch (1) actually carried
`--hostname/--port` — a daemon→port SSE conn without a confirmed launch is a stale/wrong
connection, not a real channel for this pane. Proof order is launch→listen→reachable→subscribe.
Measured now: `--via cli` → FAIL (no --port); `--via rest` → PASS (all four, ordered).
True desktop path B (`mux_spawn_pane`, Tauri IPC) is not macOS-scriptable; CLI is the proxy.
If Lead's helper is SHARED, fixing it flips `--via cli` to PASS (proves it reaches the
non-spawn_agent path).

**Server allocation WITHOUT a subscriber is not enough** (C2): if Lead centralizes
`allocate_server` but leaves `maybe_start_subscriber` orchestrator-only, the direct pane
gets a port (1–3 PASS) but structured prompts never arrive (4 FAIL) → still grid-parses →
still false-idles. The script prints `PARTIAL: allocation-without-subscriber gap` for that
case, distinct from the all-fail baseline and the all-pass success.

BEFORE (measured baseline, this round): all FOUR **FAIL** —
`cmd=opencode '-m' '<model>'` (no `--port`), no LISTEN socket (plain TUI), no control plane.
AFTER centralized fix: all three **PASS**, and the script prints `base_url` + `session` for
the stop-oracle http-abort run:
```bash
testbench/opencode-server-check.sh        # expect: SERVER ALLOCATED ON DIRECT PATH
testbench/stop-oracle.sh --pane <disposable> --mode http-abort \
    --oc-base http://127.0.0.1:<port> --oc-session ses_…    # if Lead wires abort
```
Knock-on after server is active: opencode permission/questions arrive as STRUCTURED pickers
(→ Question) instead of grid-parsed, so the opencode permission false-idle resolves via the
structured path. The synthetic `permission_box_visible_cursor_v1` (raw grid, no server) still
requires the classifier-side composer_ready guard — keep both as regression checks.

## Report shape (per loop protocol)
Report BEFORE→AFTER for each step with exact numbers, the regen dir, build hash, and
whether each is a replay (frame counts) or LIVE (pane id) measurement. WAITING_FOR_COORDINATOR
between Lead iterations.

<!-- revision: 1 -->
