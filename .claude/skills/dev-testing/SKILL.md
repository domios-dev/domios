---
name: dev-testing
description: Test CodeTerm dev instance via API. Use when testing memory, GA interactions, skills, bootstrap, recall quality. Covers pane send, mem check, worker status, file verification.
version: 0.1.0
---

# Dev Testing

Test CodeTerm dev instance (port 7686) by interacting with agents via REST API and verifying mem state.

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
