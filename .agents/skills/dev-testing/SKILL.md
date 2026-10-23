---
name: dev-testing
description: Test Domios dev instance via API and look at its UI. Use when you need to see the running dev window (screenshot), or test memory, GA interactions, skills, bootstrap, recall quality. Covers screenshots, tab send, mem check, worker status, file verification.
---

# Dev Testing

For an owner-requested integrated all-tab round, follow `evals/startup-matrix/README.md` and `EXPLORE.md`, including its event and no-loss extension; keep it local and on request.

Test Domios dev instance (port 7686) by interacting with agents via REST API and verifying mem state.

## DEV connection

Source `evals/_lib/lib.sh` and use `dc`, `dev_get`, `dev_post`, or `dev_run`.
The library resolves the selected DEV instance, its installed CLI and credential;
it scrubs inherited production connection variables. Never use a raw CLI command
or parse configuration for a credential in a bench.

The instance credential lives in `<data_dir>/.secrets`, key
`domios.instance:api_token`. The local-only `system api-token` CLI command is the
storage boundary used by the library; it refuses `--mesh` and prints only the
credential. Tabs and child processes keep the `CODETERM_API_TOKEN` environment
contract. The `$API` / `$H` setup below derives both values through the library.

| Measurement | Command and observable output |
|---|---|
| DEV lock storm | `./build.sh qa-lock-storm --seconds 120 --out target-check/lock-storm.json` (PowerShell: `./build.ps1 qa-lock-storm ...`): requires an existing tab; three independent DEV-only `dev_get` workers request mux state, tabs and one tab's chat on a 250 ms load cadence for a bounded 1..3600-second window. Reports helper-inclusive per-route p50/p95/max milliseconds and HTTP/transport status counts, plus per-lock/per-site LOCK-HOLD counts from the DEV log in JSON and a table. Slow requests never overlap on the same route; the final request can finish up to the helper's 5-second health check plus 30-second request deadline after the load window. Log timestamps have second precision, so both boundary seconds are included; unmatched older diagnostic formats are counted separately. This cadence generates load and proves no readiness state. Fixture selector: `node --test scripts/tests/qa-lock-storm.test.mjs`. |

## Setup

```bash
# Credential from the selected DEV instance
source evals/_lib/lib.sh
TOKEN="$(dev_token)"
API="$(dev_api_url)"
H="Authorization: Bearer $TOKEN"
```

## Look at the UI (screenshot)

The dev window exposes Chrome DevTools Protocol; the port comes from `/api/health` (`instance.cdp_port`, 9223 by default). One command, no Playwright:

```bash
node scripts/dev-screenshot.mjs /tmp/dev.png                       # whole window
node scripts/dev-screenshot.mjs /tmp/sidebar.png '[data-testid=general-section]'  # one element
node scripts/dev-screenshot.mjs /tmp/prod.png '' --api-port 7685   # another instance
```

It prints the file path and the running build SHA; open the PNG with your file-read/image tool to see it. Check the build SHA before judging a change: the screenshot shows what is running, not what is merged. For state the picture cannot show (counts, attributes), evaluate DOM over the same port (`Runtime.evaluate`) instead of guessing from pixels.

## Prove a wheel/scroll bug (real wheel probe)

`scripts/dev-wheel-probe.mjs` sends a real CDP `mouseWheel` (`deltaY` 240) at an element's center and reports the nearest scrollable ancestor's `scrollTop` before/after, the wheel event target and `defaultPrevented`, and the hit element's ancestor chain with computed overflow, overscroll, pointer-events and app-region. It waits on the scroll event or two rAFs, never a sleep.

```bash
node scripts/dev-wheel-probe.mjs                                   # sidebar: tab label, same row icon, empty space
node scripts/dev-wheel-probe.mjs '[data-testid=tab-title]' '.sidebar-tab-badge'   # custom points
node scripts/dev-wheel-probe.mjs '.sidebar-workspace-card[data-selected="true"] .sidebar-workspace-card-initials' '.sidebar-workspace-card-edit' '.sidebar-workspace-card[data-selected="false"] .sidebar-workspace-card-initials'
node scripts/dev-wheel-probe.mjs --api-port 7686 --delta 480
```

Exit 1 means a probe did not move its scroller: first make sure the sidebar actually overflows (open enough tabs), then compare the `chain` of the failing point with the passing one; the first node that differs is the cause.

DEV flow tracing writes bounded, rotated `~/.codeterm-dev/logs/trace.jsonl`; production writes no trace events. Unledgered PTY input uses a fresh trace ID; existing sends retain their IDs.
From a DEV-bound shell, use `domios trace recent --limit 20`, then `domios trace show <trace_id|tab_id> --since 1h` to see ordered stages and gaps in the retained window.
Exact protocol-turn and append-window source links join assistant appends, including late links; a missing assistant stage means pending or missing evidence, not a delivery verdict.

## Chat scroll trace (iPhone viewport)

For a change to Chat scrolling (`useMountedChatWindow`, `useChatScroll`, `chatSpacerDrift`, row heights), prove the scroll with a script, not by eye. The harness mounts the real hooks over 400 tool-card-sized rows. No dev instance is needed.

```bash
./build.sh chat-scroll-trace --webkit            # WebKit (no CSS scroll anchoring), fixed path: must report 0 jumps
./build.sh chat-scroll-trace --webkit --fix 0    # same scroll without the spacer drift wiring: reproduces the jumps
./build.sh chat-scroll-trace --webkit --strokes 60   # scroll all the way to the transcript top
./build.sh chat-scroll-trace                     # native anchoring (Chromium/desktop) control
```

It drives touch strokes through your Domios Browser (opened if needed; `--cdp ws://...` or `CHAT_SCROLL_TRACE_CDP` selects another CDP endpoint) at 390x844 and prints a summary JSON. It exits 1 when `visualJumps > 0` or `scrollTopReversals > 0`. A jump is on-screen motion of the first visible row that does not mirror the scrollTop change. `spacerScrollFrames` counts frames where the top spacer and scrollTop moved together (a drift fold or a window remount). Those frames are judged against the neighbouring momentum speed instead. `trace.json` and screenshots land in `--out DIR` (default: a temp dir, printed at the end).

## Find GA tab

The GA tab is protected. Find it by the protected flag:
```bash
GA=$(curl -s "$API/api/tabs" -H "$H" | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p.get('protected'): print(p['id']); break")
```

## Send message to GA

```bash
curl -s -X POST "$API/api/tabs/$GA/send" -H "$H" -H "Content-Type: application/json" \
  -d '{"text": "your message here"}'
```

## Send /new to reset GA context

```bash
curl -s -X POST "$API/api/tabs/$GA/send" -H "$H" -H "Content-Type: application/json" \
  -d '{"text": "/new"}'
sleep 5  # wait for new session
```

## Read GA output (clean)

```bash
curl -s "$API/api/tabs/$GA/output?lines=30" -H "$H" | python3 -c "
import json,sys,re
text = json.load(sys.stdin).get('output','')
clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', text)
clean = re.sub(r'\x1b\][^\x07]*(?:\x07|\x1b\\\\)', '', clean)
for line in clean.split('\n')[-20:]:
    s = line.strip()
    if s and len(s) > 3 and not s.startswith('[?'): print(s[:120])"
```

## Check mem state

```bash
# Stats
curl -s "$API/api/mem/stats" -H "$H" | python3 -m json.tool

# Search
curl -s "$API/api/mem/search?q=YOUR_QUERY&limit=5" -H "$H" | python3 -c "
import json,sys;d=json.load(sys.stdin)
for h in d.get('hits',[]):
    print(f'  {h[\"item\"][\"id\"]:50} kind={h[\"item\"][\"kind\"]:12} title={h[\"item\"][\"title\"][:40]}')"

# Get specific item
curl -s "$API/api/mem/items/ITEM_ID" -H "$H" | python3 -m json.tool

# All items (via graph)
curl -s "$API/api/mem/graph/full" -H "$H" | python3 -c "
import json,sys;d=json.load(sys.stdin)
print(f'Nodes: {len(d[\"nodes\"])}, Edges: {len(d[\"edges\"])}')
for n in sorted(d['nodes'], key=lambda x: x['id']):
    if n['kind'] not in ('kind','area'):
        print(f'  {n[\"kind\"]:12} {n[\"id\"]:50} {n.get(\"title\",\"\")[:40]}')"

# Workers
curl -s "$API/api/mem/workers" -H "$H" | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    r = (w.get('report') or '')[:80]
    print(f'  {w[\"status\"]:10} {w[\"intent\"][:50]}')
    if r: print(f'    {r}')"
```

## Check files on disk

```bash
# Identity and soul
cat ~/.codeterm-dev/general/identity.md 2>/dev/null || echo "NOT FOUND"
cat ~/.codeterm-dev/general/soul.md 2>/dev/null || echo "NOT FOUND"

# Bootstrap (should be deleted after onboarding)
ls ~/.codeterm-dev/general/bootstrap.md 2>/dev/null || echo "DELETED (good)"

# Skills
find ~/.codeterm-dev/skills/ -name "SKILL.md" 2>/dev/null

# AGENT.md (check identity injected)
head -25 ~/.codeterm-dev/general/AGENT.md
```

## Full clean restart

```bash
lsof -ti :1420 | xargs kill 2>/dev/null; lsof -ti :7686 | xargs kill 2>/dev/null; sleep 2
rm -f ~/.codeterm-dev/mem/knowledge.db ~/.codeterm-dev/mem/workers.json ~/.codeterm-dev/crons.json
rm -rf ~/.codeterm-dev/mem/blobs
rm -f ~/.codeterm-dev/general/{identity,soul,bootstrap,AGENT,CLAUDE,AGENTS}.md
# Delete GA claude session files to force fresh start (not resume)
find ~/.claude/projects/ -path "*general*" -name "*.jsonl" -delete 2>/dev/null
find ~/.claude/projects/ -path "*codeterm-dev*" -name "*.jsonl" -delete 2>/dev/null
# CRITICAL: Clear session ID from snapshot — without this GA resumes old session
sed -i '' 's/"claude_session_id": "[^"]*"/"claude_session_id": null/g' ~/.codeterm-dev/sessions/last.json
./build.sh dev
```

## Wait for ready

```bash
for i in $(seq 1 20); do
  if curl -s http://127.0.0.1:7686/api/health 2>/dev/null | grep -q ok; then echo "Ready ($i)"; break; fi
  sleep 3
done
```

## Spawn agent in another workspace

```bash
# Get workspace IDs first
curl -s "$API/api/workspaces" -H "$H" | python3 -c "
import json,sys
for ws in json.load(sys.stdin):
    print(f'{ws[\"id\"]:12} {ws[\"name\"]}')"

# Spawn claude agent with a task
curl -s -X POST "$API/api/agents/spawn" -H "$H" -H "Content-Type: application/json" \
  -d '{
    "agent": "claude",
    "workspace_id": "WORKSPACE_ID",
    "title": "test-agent",
    "task": "Your task here. Use domios mem search to find memories.",
    "skip_permissions": true
  }'
# Returns: {"tab_id": "xxx", "tab_id": "yyy", ...}
# Then check output: curl -s "$API/api/tabs/xxx/output?lines=20" -H "$H"
```

## Mem extraction workers (auto-spawn on note save)

Saving a `kind=note` triggers an auto-extraction worker. Don't call any "spawn worker" endpoint — just save and watch.

```bash
# Fire N notes that exercise the extraction pipeline
for i in $(seq 1 5); do
  curl -s -X POST "$API/api/mem/items" -H "$H" -H "Content-Type: application/json" \
    -d "{\"kind\":\"note\",\"title\":\"sanity_$i\",\"body\":\"<your prompt body — include --area --kind --tags tokens to exercise the argv-splitting fix>\",\"area\":\"codeterm\"}" &
done
wait
```

### Monitor pool state

```bash
curl -s "$API/api/mem/workers" -H "$H" | python3 -c "
import json,sys
from collections import Counter
ws=json.load(sys.stdin)
recent=[w for w in ws if 'sanity_' in w.get('intent','')]
c=Counter(w['status'] for w in recent)
print(f'n={len(recent)} {dict(c)}')"
```

States: `queued` → `starting` → `done`. Pool concurrency cap = `memory.workers_per_provider` (default 3) — you'll always see ≤3 in `starting` simultaneously. If `starting` count NEVER drops, the launch command itself is broken (regression of the stdin-pipe fix). If individual workers stay in `starting` forever, see the "stuck workers" section below.

### Read worker log

```bash
curl -s "$API/api/mem/workers/<gw_id>/log" -H "$H" | tail -c 2000
```
The log shows the actual launched shell command (`PS C:\…> Get-Content '…' | claude --dangerously-skip-permissions -p …`) followed by the captured PTY screen text. If you see `(Get-Content '…' -Raw -Encoding UTF8)` inline (the legacy form) on a hidden worker, the dev daemon was not rebuilt with the fix.

### Stuck workers

Claude in `-p` mode occasionally **prints bash code blocks as text** instead of calling the tools — the model just rambles, exits with the shell prompt back, and the worker registry never gets a `worker report` call so the row stays at `starting` forever. This is **model variance, not a launch bug** — watchdog will eventually mark them `orphaned`, or you can kill them yourself:
```bash
curl -s -X DELETE "$API/api/mem/workers/<gw_id>" -H "$H"
```
Killing also frees the semaphore slot so queued workers move forward. **Do this before reporting "fix is broken" — confirm the launched command is the new pipe form first.**

### Cleanup test items

```bash
for i in $(seq 1 5); do
  curl -s -X DELETE "$API/api/mem/items/note_sanity_$i" -H "$H" > /dev/null
done
# Delete any auto-extracted facts/learnings the workers spawned, so you don't pollute mem
```

## Cron testing (also dev-only via curl)

Cron jobs share the worker spawn path (`spawn_hidden_agent_impl`) — same launch-command bug, same fix.

### Find route handlers

```bash
# routes.rs registers POST /api/crons, POST /api/crons/{id}/trigger,
# GET /api/crons, GET /api/crons/{id}/runs, DELETE /api/crons/{id}
```

### Create + trigger via curl (NEVER `domios cron`)

```bash
# Body schema mirrors CronJob; minimum required fields:
JOB=$(curl -s -X POST "$API/api/crons" -H "$H" -H "Content-Type: application/json" -d '{
  "name":"dev-smoke",
  "schedule":"* * * * *",
  "task":"Run this once then exit: domios mem worker report \"smoke ok with --area --kind tokens\"",
  "agent":"claude",
  "model":"claude-haiku-4-5-20251001"
}' | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
echo "job=$JOB"

# Manual trigger
curl -s -X POST "$API/api/crons/$JOB/trigger" -H "$H"

# Check runs
curl -s "$API/api/crons/$JOB/runs" -H "$H" | python3 -m json.tool

# Delete when done
curl -s -X DELETE "$API/api/crons/$JOB" -H "$H"
```

### What "success" looks like for the shell-fix smoke

The tab's PTY should show the **new** launch line:
```
PS …> Remove-Item Env:CLAUDECODE …; Get-Content 'C:\…\.ct-task-XXXX' -Raw -Encoding UTF8 | claude --dangerously-skip-permissions -p --model claude-haiku-4-5-20251001
```
NOT the legacy inline `claude --dangerously-skip-permissions (Get-Content '…' -Raw -Encoding UTF8) --model …` form. If you see the legacy form on a hidden cron tab, the daemon is running stale code (build didn't pick up `shell.rs` / `provider.rs` changes — verify `target/debug/codeterm.exe` mtime is newer than the source files you edited).

Do NOT use `domios mem worker report` as the test payload of a **cron** task — cron tabs don't have `CODETERM_WORKER_ID` and the call exits 1 with `not in a worker tab`. Use a tool that doesn't depend on worker context (e.g. `domios mem save --kind note --title smoke …`) or have the task simply call `domios mem stats` and exit.

### Dreaming smoke

```bash
curl -s -X POST "$API/api/mem/dream" -H "$H" -H "Content-Type: application/json" -d '{"phase":"now"}'
# Or via the auto-installed dreaming cron — list crons, find mem-dreaming-auto, trigger it
```

## Rebuild dev daemon (only :7686, never :7685)

```bash
# Identify dev process by port — NEVER kill prod (7685)
DEV_PID=$(netstat -ano -p tcp 2>/dev/null | grep ":7686 .*LISTENING" | awk '{print $NF}')
[ -n "$DEV_PID" ] && taskkill //PID "$DEV_PID" //F   # Windows
# Then in background: bash build.sh dev
# Verify mtime: stat -c "%y %n" src-tauri/target/debug/codeterm.exe — must be newer than the source files you edited
```

Note: `bash build.sh build` runs the production frontend bundle and currently fails on pre-existing TS errors in `CronTabl.tsx` / `useConfig.ts` — those are unrelated to backend Rust work and **don't block dev testing**. Use `bash build.sh dev` (which uses `vite dev`, no strict TS check) or `cargo build --bin codeterm` directly to validate Rust changes.

## Tips

- GA tab ID stays the same across restarts (5a6a64dc in dev)
- The DEV credential is in `<dev_data_dir>/.secrets`, key `domios.instance:api_token`; resolve it with `dev_token` from the bench library.
- After sending message to GA, wait 20-60s for claude to process + extraction workers
- After spawning agent, wait 30-60s for it to start + execute task
- `/new` resets claude context but keeps mem — tests recall from fresh context
- Cross-workspace: agents in any project workspace can `domios mem search` and find GA memories
- If something doesn't work, update this skill with what you learned

## Self-improvement

If a test fails or a command doesn't work as expected, edit this SKILL.md to fix it.
The skill lives at: `.agents/skills/dev-testing/SKILL.md`

## Internal references

Moved out of the embedded `domios docs` pages because they describe Domios's own source:

- [references/memory-internals.md](references/memory-internals.md) — memory storage, ranking, wake-up selection, workers and dreaming internals

## Routed-message first-paint proof (Keel)

Run after integration in canonical DEV; Workers do not start a lane server.
Verify `/api/health` build equals the integration SHA. Use the Domios Browser
(`domios plugin browser status`, `open --profile clean`, then `goto` the DEV
web client), and select an owned, empty throwaway agent chat. Use a separate
owned DEV agent as sender; never send a probe to an owner's working chat.

Before the first routed send, open the Browser's DevTools Performance panel,
enable Screenshots, and start recording. In its console install this frame
probe while the empty target chat is selected:

```js
window.routeFrames = [];
window.routeProbeActive = true;
function routeFrame() {
  if (!window.routeProbeActive) return;
  const log = document.querySelector('[data-testid="tab-session-chat-messages"]');
  window.routeFrames.push({ at: performance.now(), text: log?.textContent ?? "" });
  requestAnimationFrame(routeFrame);
}
requestAnimationFrame(routeFrame);
```

From the DEV-locked bench shell, source `evals/_lib/lib.sh`. Set `sender` and
`target` to those two owned DEV tab IDs. Author the body in the sender's pad:

```bash
proof_pad=$(dc --as-tab "$sender" pad path)
cat > "$proof_pad/routed-proof.md" <<'EOF'
route-first-paint-opaque-token: acknowledge this token once.
EOF
dc --as-tab "$sender" send --tab "$target" --require-agent --from-pad routed-proof.md
```

Wait for the target chat to show the token and its committed transcript echo,
then stop the Performance recording and run in the console:

```js
window.routeProbeActive = false;
({ frames: routeFrames.length,
   observed: routeFrames.some(f => f.text.includes('route-first-paint-opaque-token')),
   leaks: routeFrames.filter(f => /<\/?(?:domios|tip)\b/.test(f.text)) });
```

Pass requires nonzero frames, `observed: true`, `leaks: []`, and a clean sender
card with one body throughout the screenshot filmstrip, including the first
insertion and replacement by the committed echo. Save the recording and frame
JSON in the QA pad. Reload the target and confirm one clean persisted body.
Repeat with a fresh target for each transport under review, especially a
structured runtime that publishes `chat-send-status.system_send` before its
transcript row. A send acknowledgement alone is not proof. Close only the
throwaway tabs and Browser this check created. Pure regression command:
`pnpm exec tsx src/components/Terminal/chatEnrichment.routedSends.test.ts`.

## Account adapter setup

[Account adapters](account-adapters.md) covers build-time vendor configuration,
local billing simulation and development analytics sinks.

[Instance isolation](instance-isolation.md) records development identifiers,
ports and data paths moved out of embedded product help.
