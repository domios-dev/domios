# CodeTerm Skills for Claude Code

CodeTerm ships skills that teach Claude Code how to control the terminal.

## Installation

Copy skills to your Claude Code skills directory:

```bash
cp -r skills/codeterm ~/.claude/skills/
```

Or symlink:
```bash
ln -s /path/to/codeterm/skills/codeterm ~/.claude/skills/codeterm
```

## Available Skills

### `codeterm` — Core terminal control
- Create tabs, manage workspaces
- Send commands to other panes
- List sessions and panes

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `$CODETERM=1` env var and `$CODETERM_PANE_ID` for self-identification), it can use the `codeterm` CLI to:

1. **Send commands**: `codeterm send-text "npm test"` (sends to current pane, newline appended automatically)
2. **Send notifications**: `codeterm notify "Done" --level info`
3. **Create workspaces**: `codeterm workspace create "Feature X"`
4. **Read output**: `codeterm get-output` (reads from current pane)
5. **Rename tab**: `codeterm pane rename "my task"` or `codeterm pane rename "my task" --pane ID`
6. **Create theme**: `codeterm theme save my-theme /tmp/theme.toml --apply`
7. **Agent hierarchy**: `codeterm agent set-conductor` (mark self as orchestrator; alias: `set-orchestrator`)
8. **Compact context**: `codeterm agent compact [--pane ID]` (sends /compact to Claude)
9. **Switch model**: `codeterm agent model <sonnet|haiku|opus> [--pane ID]` (switches Claude model)
9. **Send report**: `codeterm agent report "task done"` (report to direct parent, auto-injected into parent's PTY)
10. **List hierarchy**: `codeterm agent hierarchy` (show full agent tree)
11. **Spawn agent**: `codeterm agent spawn claude --title "Worker" --role Worker --parent ID --task "do work"` (creates new tab, non-blocking, does NOT switch active tab, task uses `-p` flag for instant start)
12. **Spawn with agent definition**: via REST API `POST /api/agents/spawn` with `agent_name` field (loads `.claude/agents/<name>.md`; not available in CLI)
13. **Cross-workspace spawn**: `codeterm agent spawn claude --workspace WS_ID --title "Remote" --role Worker --parent ID --task "work"`
14. **Create cron job**: `codeterm cron create "name" --schedule "0 9 * * *" --task "Review TODOs"` (uses `$CODETERM_WORKSPACE_ID` by default; add `--once` for oneshot: `--schedule "in 30m" --once`)
15. **Edit cron job**: `codeterm cron edit ID --schedule "*/15 * * * *"` (prefer over delete+recreate to preserve history)
16. **Complete cron run**: `codeterm cron complete --run-id UUID --summary "result"`
17. **List cron jobs**: `codeterm cron list`, **trigger manually**: `codeterm cron trigger ID`
18. **Read config**: `codeterm config get integrations.telegram_enabled` (dot-notation)
19. **Write config**: `codeterm config set integrations.telegram_enabled true` (JSON values)
20. **Send to Telegram**: `codeterm bridge send --to USER_ID --service telegram "text"` (auto-edits within 60s, `--append` to concatenate)
21. **Keep system awake**: `codeterm keepawake start --duration-secs 3600 [--display]` (cross-platform: caffeinate on macOS, SetThreadExecutionState on Windows). Stop: `codeterm keepawake stop`. Status: `codeterm keepawake status`. Use this instead of calling `caffeinate` / `powercfg` / `systemd-inhibit` directly.
22. **Search past agent sessions**: `codeterm sessions search "QUERY" [--provider claude|codex|all] [--project /path] [--limit N]` — content search across every registered provider's on-disk sessions. Each hit carries its `provider` and `session_id`.
23. **Inspect a past session**: `codeterm sessions get SESSION_ID` — provider, recorded cwd, file path, timestamps. Use before `resume` to verify the JSONL is still on disk.
24. **Resume a past session atomically**: `codeterm sessions resume SESSION_ID [--title T] [--workspace-id ID] [--workspace-name N]` — locates the session, opens the right workspace at the recorded cwd, spawns the agent pane, and hands the resume id to the provider with the correct CLI shape. Use this instead of the brittle "create workspace at guessed cwd → `agent spawn --args=--resume=UUID`" dance.

## CLI Flags

All CLI commands support the `--json` flag for machine-readable output:
```bash
codeterm pane list --json
codeterm workspace list --json
```

## Integrations

### Telegram Bridge
CodeTerm routes messages between Telegram and the General Agent:
- **Inbound:** Messages arrive as `[Telegram: user_id] text` in the agent's PTY. Rapid messages within 300ms are debounced. Typing indicator is sent automatically.
- **Outbound:** Agent sends replies via CLI: `codeterm bridge send --to USER_ID --service telegram "text"`. Within 60s, messages auto-edit the previous one. Use `--append` to concatenate text.

Setup via CLI:
```bash
# Save token to secrets dir
mkdir -p ~/.codeterm/secrets
echo -n "BOT_TOKEN" > ~/.codeterm/secrets/telegram.token

# Enable
codeterm config set integrations.telegram_enabled true
codeterm config set integrations.telegram_allowed_users '["USER_ID"]'

# Restart CodeTerm (hot-reload not yet supported)
```

Sending messages from the agent:
```bash
# Send a new message
codeterm bridge send --to 123456789 --service telegram "Hello from CodeTerm"

# Append to the previous message (edits in-place)
codeterm bridge send --to 123456789 --service telegram --append "...continued"
```

Connected panes show service icons in the sidebar (`connected_services` field).

### Config Management
Read/write CodeTerm config at runtime:
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent                    # read entire section
```
Dot-notation for nested keys. Values are JSON (true, false, 42, "string", ["array"]).

## Agent Integration

External agents (Rollo, OpenClaw, CI/CD) can connect via REST API:

```bash
# Create workspace for agent
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"Agent"}'

# Create tab
curl -X POST http://localhost:7685/api/workspaces/$WS_ID/tabs

# Send command (enter: true is default, appends \r automatically)
curl -X POST http://localhost:7685/api/panes/$PANE_ID/send \
  -H "Content-Type: application/json" \
  -d '{"text":"npm run build"}'

# Read output
curl http://localhost:7685/api/panes/$PANE_ID/output

# Notify
curl -X POST http://localhost:7685/api/notifications \
  -H "Content-Type: application/json" \
  -d '{"title":"Build complete","level":"info"}'
```

## Agent Hierarchy

CodeTerm supports a 3-level agent hierarchy: Orchestrator (depth 1) -> Lead (depth 2) -> Worker (depth 2-3). Each agent gets its own tab; the hierarchy is a logical chain, not visual splits. IDs are 8-char hex (e.g. `a3f1b2c7`).

**Roles:**
- **Orchestrator** (was Conductor) -- top-level, can spawn Leads and Workers.
- **Lead** -- spawned by Orchestrator, can spawn Workers.
- **Worker** -- executes tasks, reports back. Cannot spawn. Cannot call `set-conductor`, `unlink`, or `agent spawn`.

Backward-compat aliases: `Conductor` -> `Orchestrator`, `SubWorker` -> `Worker`.

**Push-based reports:** Reports go to direct parent (not orchestrator), enabling cascade: Worker -> Lead -> Orchestrator. Reports are auto-injected into the parent's PTY as `[Report from Name] message\r`, so Claude sees them as user input -- no polling needed.

```bash
# Mark current pane as orchestrator
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# View full hierarchy tree
codeterm agent hierarchy

# List all agent panes
codeterm agent list

# Get detailed status for a pane
codeterm agent status --pane $WORKER

# Remove role (Orchestrator only -- Workers/Leads with parents CANNOT unlink)
codeterm agent unlink --pane $CODETERM_PANE_ID
```

The `agent.report` webhook event fires when any agent sends a report, enabling async orchestration patterns.

### One-call agent spawn

The `task` field is baked into the launch command via the `-p` flag, so the agent starts working immediately with no delay. Spawn returns immediately (non-blocking) -- PTY readiness and agent launch happen in a background thread. Each spawned agent gets its own tab (does NOT auto-switch active tab). Uses socket API for reliability.

```bash
# Spawn a Lead under Orchestrator
codeterm agent spawn claude --title "Team-A" --role Lead --parent $CODETERM_PANE_ID --cwd /project --task "coordinate team"

# Spawn a Worker
codeterm agent spawn claude --title "Worker-1" --role Worker --parent $CODETERM_PANE_ID --task "run tests"

# Spawn defaults to claude if no agent specified
codeterm agent spawn --title "Helper" --role Worker --parent $CODETERM_PANE_ID

# Cross-workspace spawn
codeterm agent spawn claude --workspace WS_ID --title "Remote" --role Worker --parent $CODETERM_PANE_ID
```

Returns: pane_id, tab_id, pty_id, agent, title, role.

The `--agent-name` option (for loading `.claude/agents/<name>.md`) is only available via the REST API (`POST /api/agents/spawn`), not the CLI.

### Batch API (REST and Tauri IPC)

Spawn an entire agent hierarchy in one call. The batch API is available via both `POST /api/batch` (REST) and the `mux_batch` Tauri IPC command, with identical semantics and $ref resolution. Both delegate to the unified ServiceContext.

```bash
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "can_spawn": true}},
      {"method": "spawn", "id": "lead", "params": {"title": "Lead-A", "role": "Lead", "parent_pane_id": "$boss", "task": "coordinate team"}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_pane_id": "$lead", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_pane_id": "$boss", "task": "run tests"}}
    ]
  }'
```

Available batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`. Use `$id` references to chain command results (e.g. `"parent_pane_id": "$boss"` resolves to the pane_id from the "boss" spawn).

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, splits, settings, and theme switching.

## Session Persistence

CodeTerm tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per pane on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`. Before injecting the resume flag, CodeTerm validates the session file exists on disk via `AgentProvider::session_exists` — missing JSONLs (cleared cache, machine-synced config without sessions) launch fresh instead of looping on `No conversation found with session ID`.
- **Pre-generated session ids**: For providers reporting `SessionIdInjection != None` (Claude today), CodeTerm pre-generates a UUID and passes it via `--session-id` at launch. Each pane gets a unique id even when several panes share the same cwd.
- **SessionStart hook**: Claude's `SessionStart` matcher POSTs back to `/api/panes/{id}/hook/session_start`, so `/new` and `/clear` inside the TUI rotate the persisted `agent_session_id` immediately.
- **Cross-provider session search**: `codeterm sessions search` walks every registered provider's on-disk sessions (Claude JSONLs, Codex `~/.codex/sessions/YYYY/MM/DD/*.jsonl`). `codeterm sessions resume` opens the pane in the correct cwd and hands the resume id to the right provider.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 20 -->
