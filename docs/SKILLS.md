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
- Split panes, create tabs, manage workspaces
- Send commands to other panes
- List sessions and panes

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `$CODETERM=1` env var and `$CODETERM_PANE_ID` for self-identification), it can use the `codeterm` CLI to:

1. **Split panes**: `codeterm pane split` (splits current pane)
2. **Send commands**: `codeterm send-text "npm test"` (sends to current pane, newline appended automatically)
3. **Send notifications**: `codeterm notify "Done" --level info`
4. **Create workspaces**: `codeterm workspace create "Feature X"`
5. **Read output**: `codeterm get-output` (reads from current pane)
6. **Rename tab**: `codeterm pane rename "my task"` or `codeterm pane rename "my task" --pane ID`
7. **Create theme**: `codeterm theme save my-theme /tmp/theme.toml --apply`
8. **Agent hierarchy**: `codeterm agent set-conductor` (mark self as conductor)
9. **Send report**: `codeterm agent report "task done"` (report to parent conductor)
10. **List hierarchy**: `codeterm agent hierarchy` (show full agent tree)
11. **Spawn agent**: `codeterm agent spawn claude --title "Worker" --role Worker --parent ID --task "do work"` (creates new tab, non-blocking, task uses `-p` flag for instant start)
12. **Spawn with agent definition**: via REST API `POST /api/agents/spawn` with `agent_name` field (loads `.claude/agents/<name>.md`; not available in CLI)

## Screenshots for Debugging

The `codeterm screenshot` CLI command captures the CodeTerm window as a JPEG image, even when the window is behind other windows. This is useful for visual debugging and verifying UI state.

```bash
# Capture and print JSON (data URL + width/height/size_kb)
codeterm screenshot

# Save directly to file
codeterm screenshot --output /tmp/screenshot.jpg
```

The REST API equivalent is `GET /api/screenshot` (returns JSON) or `GET /api/screenshot?format=jpeg` (raw JPEG bytes).

**Recommended pattern:** save the screenshot to a file, then use a **subagent** to read and analyze the image. This avoids polluting your main context with large base64 data.

```bash
# Save screenshot
codeterm screenshot --output /tmp/debug.jpg

# Analyze in a subagent (keeps main context clean)
# The subagent can use its Read tool to view /tmp/debug.jpg
```

## CLI Flags

All CLI commands support the `--json` flag for machine-readable output:
```bash
codeterm pane list --json
codeterm workspace list --json
```

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

CodeTerm supports a 3-level agent hierarchy: Conductor (depth 1) -> Worker (depth 2) -> SubWorker (depth 3). Each agent gets its own tab; the hierarchy is a logical chain, not visual splits. IDs are 8-char hex (e.g. `a3f1b2c7`).

```bash
# Mark current pane as conductor (use $CODETERM_PANE_ID for self-identification)
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# Split a pane and link it as a worker
codeterm pane split
WORKER=$(codeterm pane current --last)
codeterm agent link --parent $CODETERM_PANE_ID --pane $WORKER

# Worker sends report to its conductor
codeterm agent report "build passed" --pane $WORKER

# Conductor reads reports
codeterm agent reports

# View full hierarchy tree
codeterm agent hierarchy

# List all agent panes
codeterm agent list

# Get detailed status for a pane
codeterm agent status --pane $WORKER

# Allow a worker to spawn sub-workers
codeterm agent allow-spawn --pane $WORKER

# Remove role from a pane
codeterm agent unlink --pane $WORKER
```

The `agent.report` webhook event fires when workers send reports, enabling async orchestration patterns.

### One-call agent spawn

The `task` field is baked into the launch command via the `-p` flag, so the agent starts working immediately with no delay. Spawn returns immediately (non-blocking) -- PTY readiness and agent launch happen in a background thread. Each spawned agent gets its own tab.

```bash
# Spawn a Claude worker linked to a conductor
codeterm agent spawn claude --title "Worker-1" --role Worker --parent $CODETERM_PANE_ID --cwd /project --task "run tests"

# Spawn defaults to claude if no agent specified
codeterm agent spawn --title "Helper" --role Worker --parent $CODETERM_PANE_ID

# Workers need allow-spawn before spawning SubWorkers
codeterm agent allow-spawn --pane <WORKER_PANE_ID>
codeterm agent spawn claude --title "Sub" --role SubWorker --parent <WORKER_PANE_ID> --task "subtask"
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
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Conductor", "can_spawn": true}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_pane_id": "$boss", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_pane_id": "$boss", "task": "run tests"}}
    ]
  }'
```

Available batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`. Use `$id` references to chain command results (e.g. `"parent_pane_id": "$boss"` resolves to the pane_id from the "boss" spawn).

## Session Persistence

CodeTerm tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per pane on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`, so context is preserved across restarts.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 10 -->
