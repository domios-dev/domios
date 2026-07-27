# Agent-restart testing playbook

How to test the `codeterm agent restart` flow against a dev instance —
both the graceful path (default) and the forced path (`--force`).

Cross-references:
- Service code: `src-tauri/src/service/agent_restart.rs`
- REST handler: `src-tauri/src/api/routes.rs::agent_restart_handler`
- CLI: `cli/src/main.rs::Agent { Restart { ... } }`
- API doc: [`docs/API.md` §Agent Restart](../API.md#agent-restart-post-apipanesidagentrestart)

## What the flow does

The restart reuses the existing `/exit` + auto-resume Tab lifecycle —
the same code path that powers in-tab `/exit` and the `update install --auto`
post-install relaunch. No bespoke restart logic. Steps:

1. Lookup the Tab and provider (Claude / Codex / ...).
2. Send the provider's exit command (Claude: `/exit`, etc.).
3. Wait up to `grace_secs` (default 10) for the PTY to exit cleanly.
4. If `--force` is set OR the timeout fires, kill the PTY.
5. The existing Tab lifecycle detects the exit and respawns the agent
   with `--resume <session_id>` if a session id was recorded — otherwise
   a fresh session is started.

## Setup

Run a dev instance with at least one Claude Tab so there's an agent to
target:

```bash
./build.sh dev
# Open a workspace, spawn `claude` in a pane, let it idle
PANE=$(curl -s http://127.0.0.1:7686/api/panes \
  -H "Authorization: Bearer $TOKEN" \
  | python3 -c "import sys,json;print([p['id'] for p in json.load(sys.stdin) if p.get('agent_type')=='claude'][0])")
echo "Target pane: $PANE"
```

## 1. Graceful restart

```bash
~/.codeterm-dev/bin/codeterm agent restart --tab $PANE
```

Expected:
- The Tab prints whatever `/exit` produces (Claude shows a goodbye
  banner), then the PTY closes.
- Within ~1s the same Tab respawns with a new PID; if the previous
  session had a `claude_session_id` recorded, the new process is
  launched with `--resume <id>`. Verify with:
  ```bash
  curl -s http://127.0.0.1:7686/api/panes -H "Authorization: Bearer $TOKEN" \
    | python3 -c "import sys,json; p=[x for x in json.load(sys.stdin) if x['id']==\"$PANE\"][0]; print(p)"
  ```
  `pty_id` should change; `claude_session_id` should remain stable.

## 2. Forced restart on a hung agent

To simulate a hang, run a Claude Tab and `Ctrl-Z` it (Unix) or send a
SIGSTOP to the PTY's child process (Windows: use Process Explorer to
suspend the process). Then:

```bash
~/.codeterm-dev/bin/codeterm agent restart --tab $PANE --force --grace-secs 3
```

Expected:
- The graceful exit times out (the suspended process won't respond),
  the PTY is killed at the 3s mark.
- The Tab respawns. `session_was_stale` in the response indicates
  whether the recorded session id was valid; if the JSONL was deleted
  between runs, the field is `true` and the new Tab starts a fresh
  session.

## 3. Rate-limit guard

The service throttles restarts per-tab to avoid restart loops. Hit
the endpoint twice in quick succession:

```bash
for i in 1 2; do
  curl -i -X POST "http://127.0.0.1:7686/api/panes/$tab/agent/restart" \
    -H "Authorization: Bearer $TOKEN"
done
```

The second call returns `429 Too Many Requests` with a `Retry-After`
header. Wait for the cooldown, retry, and the call should succeed.

## 4. Error cases

- **`404`** — Tab id doesn't exist (typo, closed Tab).
- **`400`** — Tab is a plain shell with no agent, or the provider
  doesn't support restart.
- **`500`** — internal failure (PTY spawn error). Check
  `~/.codeterm-dev/logs/codeterm.log` for the trace.

## 5. Use from another agent

A common pattern: an Orchestrator notices a Worker is stuck and asks
the system to restart it.

```bash
codeterm agent restart --tab $WORKER_PANE_ID --grace-secs 30
```

The Orchestrator's Tab keeps running; only the Worker's PTY cycles.
Reports the Worker had already sent are preserved (the in-memory
`ReportStore` is not tab-scoped to PTY id; it's keyed on the parent +
child Tab ids which don't change across restart).
