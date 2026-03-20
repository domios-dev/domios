# CodeTerm Skills for Claude Code

CodeTerm ships skills that teach Claude Code how to control the terminal.

## Installation

Copy skills to your Claude Code skills directory:

```bash
cp -r skills/codeterm ~/.claude/skills/
cp -r skills/codeterm-notify ~/.claude/skills/
cp -r skills/codeterm-canvas ~/.claude/skills/
cp -r skills/codeterm-layout ~/.claude/skills/
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

### `codeterm-notify` — Notifications
- Send info/warning/error notifications
- Useful for task completion reports

### `codeterm-canvas` — Visual canvas (future)
- Render dashboards, diagrams in browser panel
- A2UI component rendering

### `codeterm-layout` — Layout management
- Save/restore pane layouts
- Common layouts: side-by-side, three-panel, quad

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `CODETERM=1` env var), it can use the `codeterm` CLI to:

1. **Split panes**: `codeterm pane split $PANE_ID --direction right`
2. **Send commands**: `codeterm send-text $PANE_ID "npm test"`
3. **Send notifications**: `codeterm notify "Done" --level info`
4. **Create workspaces**: `codeterm workspace create "Feature X"`
5. **Read output**: `codeterm get-output $PANE_ID`

## Agent Integration

External agents (Rollo, a gateway, CI/CD) can connect via REST API:

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
