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

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `CODETERM=1` env var), it can use the `codeterm` CLI to:

1. **Split panes**: `codeterm pane split` (splits current pane)
2. **Send commands**: `codeterm send-text "npm test"` (sends to current pane, newline appended automatically)
3. **Send notifications**: `codeterm notify "Done" --level info`
4. **Create workspaces**: `codeterm workspace create "Feature X"`
5. **Read output**: `codeterm get-output` (reads from current pane)
6. **Rename tab**: `codeterm pane rename "my task"` (renames current tab)
7. **Create theme**: `codeterm theme save my-theme /tmp/theme.toml --apply`
8. **Agent hierarchy**: `codeterm agent set-conductor` (mark self as conductor)
9. **Send report**: `codeterm agent report "task done"` (report to parent conductor)
10. **List hierarchy**: `codeterm agent hierarchy` (show full agent tree)

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

# Send command
curl -X POST http://localhost:7685/api/panes/$PANE_ID/send \
  -H "Content-Type: application/json" \
  -d '{"text":"npm run build\r\n"}'

# Read output
curl http://localhost:7685/api/panes/$PANE_ID/output

# Notify
curl -X POST http://localhost:7685/api/notifications \
  -H "Content-Type: application/json" \
  -d '{"title":"Build complete","level":"info"}'
```

## Agent Hierarchy

CodeTerm supports a 3-level agent hierarchy: Conductor (depth 1) -> Worker (depth 2) -> SubWorker (depth 3).

```bash
# Mark current pane as conductor
codeterm agent set-conductor

# Split a pane and link it as a worker
codeterm pane split
WORKER=$(codeterm pane current --last)
codeterm agent link --parent $CONDUCTOR_ID --pane $WORKER

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

## Session Persistence

CodeTerm tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per pane on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`, so context is preserved across restarts.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 5 -->
