---
name: dev-testing
description: Test CodeTerm dev instance via API. Use when testing memory, GA interactions, skills, bootstrap, recall quality. Covers pane send, mem check, worker status, file verification.
version: 0.2.0
---

# Dev Testing

Test CodeTerm dev instance (port 7686) by interacting with agents via REST API and verifying mem state.

## ⚠ Hard rule: never `codeterm` CLI for dev work

The `codeterm` CLI **does not** read `CODETERM_API_URL` / `CODETERM_API_TOKEN`. It picks dev vs prod by:
1. `CODETERM_SOCKET_PATH` env var — if it contains `codeterm-dev`, use dev (`~/.codeterm-dev/config.toml` → port 7686).
2. Otherwise → **prod** (`~/.codeterm/config.toml` → port 7685).

Plain `codeterm cron create / trigger / mem save` from a regular shell will hit your **prod CodeTerm** desktop daemon and spawn panes inside the user's running app. **This is destructive — visible to the user, mutates their prod state, may burn API quota under their account.**

**Always do dev testing via raw `curl` to `http://127.0.0.1:7686` with the dev token from `~/.codeterm-dev/config.toml`.** Use the `$API` / `$H` setup below — every command in this skill assumes them.

If you absolutely need the CLI for a specific subcommand, prefix with both env vars:
```bash
CODETERM_SOCKET_PATH=/fake/codeterm-dev/sock CODETERM_API_PORT=7686 codeterm <cmd>
```
The fake socket path just has to contain the substring `codeterm-dev` so the CLI picks the dev config dir; the actual socket connection is only used by a few subcommands.

## Setup

```bash
# Token from config
TOKEN=$(grep -A1 '^\[api\]' ~/.codeterm-dev/config.toml | grep token | sed 's/.*= *"\(.*\)"/\1/')
API="http://127.0.0.1:7686"
H="Authorization: Bearer $TOKEN"
```

## Find GA pane

GA pane is protected. Find by protected flag:
```bash
GA=$(curl -s "$API/api/panes" -H "$H" | python3 -c "
import json,sys
for p in json.load(sys.stdin):
    if p.get('protected'): print(p['id']); break")
```

## Send message to GA

```bash
curl -s -X POST "$API/api/panes/$GA/send" -H "$H" -H "Content-Type: application/json" \
  -d '{"text": "your message here"}'
```

## Send /new to reset GA context

```bash
curl -s -X POST "$API/api/panes/$GA/send" -H "$H" -H "Content-Type: application/json" \
  -d '{"text": "/new"}'
sleep 5  # wait for new session
```

## Read GA output (clean)

```bash
curl -s "$API/api/panes/$GA/output?lines=30" -H "$H" | python3 -c "
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
    "task": "Your task here. Use codeterm mem search to find memories.",
    "skip_permissions": true
  }'
# Returns: {"pane_id": "xxx", "tab_id": "yyy", ...}
# Then check output: curl -s "$API/api/panes/xxx/output?lines=20" -H "$H"
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

### Create + trigger via curl (NEVER `codeterm cron`)

```bash
# Body schema mirrors CronJob; minimum required fields:
JOB=$(curl -s -X POST "$API/api/crons" -H "$H" -H "Content-Type: application/json" -d '{
  "name":"dev-smoke",
  "schedule":"* * * * *",
  "task":"Run this once then exit: codeterm mem worker report \"smoke ok with --area --kind tokens\"",
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

The pane's PTY should show the **new** launch line:
```
PS …> Remove-Item Env:CLAUDECODE …; Get-Content 'C:\…\.ct-task-XXXX' -Raw -Encoding UTF8 | claude --dangerously-skip-permissions -p --model claude-haiku-4-5-20251001
```
NOT the legacy inline `claude --dangerously-skip-permissions (Get-Content '…' -Raw -Encoding UTF8) --model …` form. If you see the legacy form on a hidden cron pane, the daemon is running stale code (build didn't pick up `shell.rs` / `provider.rs` changes — verify `target/debug/codeterm.exe` mtime is newer than the source files you edited).

Do NOT use `codeterm mem worker report` as the test payload of a **cron** task — cron panes don't have `CODETERM_WORKER_ID` and the call exits 1 with `not in a worker pane`. Use a tool that doesn't depend on worker context (e.g. `codeterm mem save --kind note --title smoke …`) or have the task simply call `codeterm mem stats` and exit.

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

Note: `bash build.sh build` runs the production frontend bundle and currently fails on pre-existing TS errors in `CronPanel.tsx` / `useConfig.ts` — those are unrelated to backend Rust work and **don't block dev testing**. Use `bash build.sh dev` (which uses `vite dev`, no strict TS check) or `cargo build --bin codeterm` directly to validate Rust changes.

## Tips

- GA pane ID stays the same across restarts (5a6a64dc in dev)
- Token is in `~/.codeterm-dev/config.toml` under `[api] token = "..."`
- After sending message to GA, wait 20-60s for claude to process + extraction workers
- After spawning agent, wait 30-60s for it to start + execute task
- `/new` resets claude context but keeps mem — tests recall from fresh context
- Cross-workspace: agents in Binaura/codeterm can `codeterm mem search` and find GA memories
- If something doesn't work, update this skill with what you learned

## Self-improvement

If a test fails or a command doesn't work as expected, edit this SKILL.md to fix it.
The skill lives at: `/Users/rollacode/Developer/codeterm/.claude/skills/dev-testing/SKILL.md`
