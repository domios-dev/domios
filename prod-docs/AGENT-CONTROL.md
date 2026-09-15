# Agent control

Skill-driven agent integration, hierarchy, command palette, and session persistence.

## Agent Integration

External agents (personal assistants, gateways, CI/CD) can connect via REST API:

```bash
# Create workspace for agent
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"Agent"}'

# Create tab
curl -X POST http://localhost:7685/api/workspaces/$WS_ID/tabs

# Send command to a tab (enter: true is default, appends \r automatically)
curl -X POST http://localhost:7685/api/tabs/$TAB_ID/send \
  -H "Content-Type: application/json" \
  -d '{"text":"npm run build"}'

# Press terminal keys / navigation actions
curl -X POST http://localhost:7685/api/tabs/$TAB_ID/send \
  -H "Content-Type: application/json" \
  -d '{"actions":[{"type":"key","key":"esc"},{"type":"delay","ms":200},{"type":"key","key":"enter"}]}'

# Read tab output
curl http://localhost:7685/api/tabs/$TAB_ID/output

# Notify
curl -X POST http://localhost:7685/api/notifications \
  -H "Content-Type: application/json" \
  -d '{"title":"Build complete","severity":"info","level":"action_needed"}'
```

## Agent Hierarchy

CodeTerm has a 2-level role hierarchy: Orchestrator (depth 1) -> Worker (depth 2). Each agent gets its own tab; the hierarchy is a logical chain, not a nested layout. IDs are 8-char hex (e.g. `a3f1b2c7`).

**Roles:**
- **Orchestrator** (was Conductor) -- top-level, spawns Workers.
- **Worker** -- executes one task, reports back. Cannot spawn. Cannot call `set-orchestrator`, `unlink`, or `agent spawn`.

Backward-compat alias: `Conductor` -> `Orchestrator`.

**Push-based reports:** Reports go to the spawning Orchestrator, auto-injected into its PTY as `[Report from Name] message\r`, so Claude sees them as user input -- no polling needed.

```bash
# Mark current tab as orchestrator
codeterm agent set-orchestrator --tab $CODETERM_TAB_ID

# View the complete local workspace tree (sub-agents nested); --workspace selects one workspace; --agents = agent tabs only
codeterm tab list

# Get one tab's agent state (type, session, prompt)
codeterm tab status --tab $WORKER

# Repair a wrong live provider label from the tab's verified session, no restart
codeterm tab repair --from-evidence

# The Plan owner can repair an assigned worker through the same evidence boundary
codeterm tab repair --from-evidence --tab $WORKER

TEAM_ID=$CODETERM_TEAM_ID
# Clear role; a final Orchestrator retains its Team and active Plan for replacement
codeterm agent unlink --tab $CODETERM_TAB_ID

# Replace that missing Orchestrator while preserving the Team and Plan history
codeterm agent spawn claude --team "$TEAM_ID" --role Orchestrator
```

The `agent.report` webhook event fires when any agent sends a report, enabling async orchestration patterns.

### One-call agent spawn

The `task` field is baked into the launch command via the `-p` flag, so the agent starts working immediately with no delay. Spawn returns immediately (non-blocking) -- PTY readiness and agent launch happen in a background thread. Each spawned agent gets its own tab (does NOT auto-switch active tab). Uses socket API for reliability.

```bash
# Spawn a Worker under the Orchestrator
codeterm agent spawn claude --title "Worker-1" --role Worker --parent-tab $CODETERM_TAB_ID --task "run tests"

# Spawn defaults to claude if no agent specified
codeterm agent spawn --title "Helper" --role Worker --parent-tab $CODETERM_TAB_ID

# Cross-workspace spawn
codeterm agent spawn claude --workspace WS_ID --title "Remote" --role Worker --parent-tab $CODETERM_TAB_ID
```

Returns: tab_id, tab_id, pty_id, agent, title, role.

The `--agent-name` option (for loading `.claude/agents/<name>.md`) is only available via the REST API (`POST /api/agents/spawn`), not the CLI.

### Batch API (REST and Tauri IPC)

Spawn an Orchestrator and its Workers in one call. The batch API is available via both `POST /api/batch` (REST) and the `mux_batch` Tauri IPC command, with identical semantics and $ref resolution. Both delegate to the unified ServiceContext.

```bash
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "can_spawn": true}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_tab_id": "$boss", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_tab_id": "$boss", "task": "run tests"}}
    ]
  }'
```

Available batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`. Use `$id` references to chain command results (e.g. `"parent_tab_id": "$boss"` resolves to the tab_id from the "boss" spawn).

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, settings, and theme switching.

## Session Persistence

CodeTerm tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per tab on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`. Before injecting the resume flag, CodeTerm validates the session file exists on disk via `AgentProvider::session_exists` — missing JSONLs (cleared cache, machine-synced config without sessions) launch fresh instead of looping on `No conversation found with session ID`.
- **Pre-generated session ids**: For providers reporting `SessionIdInjection != None` (Claude today), CodeTerm pre-generates a UUID and passes it via `--session-id` at launch. Each tab gets a unique id even when several tabs share the same cwd.
- **SessionStart hook**: Claude's `SessionStart` matcher POSTs back to `/api/tabs/{id}/hook/session_start`, so `/new` and `/clear` inside the TUI rotate the persisted `agent_session_id` immediately.
- **Cross-provider session search**: `codeterm sessions search` walks every registered provider's on-disk sessions (Claude JSONLs, Codex `~/.codex/sessions/YYYY/MM/DD/*.jsonl`). `codeterm sessions resume` opens the tab in the correct cwd and hands the resume id to the right provider.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 7 -->
