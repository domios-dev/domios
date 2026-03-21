---
name: codeterm
description: Control CodeTerm terminal multiplexer — workspaces, panes, browser panels, notifications, layouts. Use when user mentions splitting panes, creating workspaces, sending commands, visualizations, or managing terminal sessions.
version: 0.2.0
---

# CodeTerm — AI Terminal Control

You are running inside CodeTerm, an AI-first terminal multiplexer. CLI: `C:/tools/codeterm.exe`

## Auto-name your tab on session start

**Immediately rename your tab** based on the user's first message/task:
```bash
PANE_ID=$(C:/tools/codeterm.exe pane list --json | jq -r '.[0].id')
C:/tools/codeterm.exe pane rename "$PANE_ID" "fix login bug"
```

## Terminal Management
```
codeterm pane list                              # List panes
codeterm pane split PANE_ID --direction right   # Split horizontal
codeterm pane split PANE_ID --direction down    # Split vertical
codeterm pane focus PANE_ID                     # Focus pane
codeterm pane close PANE_ID                     # Close pane
codeterm pane rename PANE_ID "title"            # Rename tab
codeterm pane resize PANE_ID --cols 120 --rows 40  # Resize terminal
codeterm send-text PANE_ID "command"            # Send to terminal
codeterm send-text PANE_ID "command" --enter    # Send + press Enter (default: true)
codeterm send-text PANE_ID "cmd1" "cmd2" "cmd3" # Batch: send multiple commands
codeterm get-output PANE_ID                     # Read output
```

## Workspace Management
```
codeterm workspace list                         # List workspaces
codeterm workspace create "Name"                # Create workspace
codeterm workspace focus ID                     # Switch workspace
codeterm workspace rename ID "New Name"         # Rename
codeterm workspace close ID                     # Delete
codeterm workspace move-tab TAB --from WS --to WS  # Move tab
```

## Browser Panel (Canvas)

Render HTML visualizations INSIDE CodeTerm. NEVER open external browser.

```bash
# Write HTML to file, then open
cat > /tmp/viz.html << 'EOF'
<!DOCTYPE html>
<html><head><style>
body { background: #0d1117; color: #c9d1d9; font-family: sans-serif; padding: 20px; }
</style></head><body><h1>Hello</h1></body></html>
EOF
C:/tools/codeterm.exe browser open file:///tmp/viz.html
```

Also works with: `data:text/html,<h1>Hello</h1>`, `http://localhost:PORT`

## Notifications
```
codeterm notify "Title" --level info --body "Details"
codeterm notify "Error" --level error --body "Build failed"
codeterm notifications --unread
codeterm notifications clear
```

Levels: `info` (blue), `warning` (yellow), `error` (red)

## Common Layouts

### Side-by-side
```bash
PANE=$(codeterm pane list --json | jq -r '.[0].id')
codeterm pane split $PANE --direction right
```

### Three-panel
```bash
PANE=$(codeterm pane list --json | jq -r '.[0].id')
codeterm pane split $PANE --direction right
RIGHT=$(codeterm pane list --json | jq -r '.[-1].id')
codeterm pane split $RIGHT --direction down
```

## Multi-agent Workflow
```bash
codeterm workspace create "frontend"
codeterm workspace create "backend"
# Each workspace gets its own Claude Code instance
```

## Session Persistence

Sessions auto-save every 30s. CWD is tracked via OSC 7 and restored per pane. Claude Code sessions are detected and auto-resumed with `--resume`.

## System
```
codeterm identify        # Version & features
codeterm capabilities    # All API methods
```
