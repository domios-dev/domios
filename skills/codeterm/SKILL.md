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
C:/tools/codeterm.exe pane rename "fix login bug"
```

## Terminal Management

Commands default to the **current focused pane** when no ID is given.

```
codeterm pane list                              # List panes
codeterm pane current                           # Get current pane ID
codeterm pane current --last                    # Get last pane ID
codeterm pane split                             # Split current pane right
codeterm pane split --direction down            # Split current pane down
codeterm pane split --id PANE_ID                # Split specific pane
codeterm pane rename "title"                    # Rename current tab
codeterm pane focus PANE_ID                     # Focus pane
codeterm pane close PANE_ID                     # Close pane
codeterm send-text "command"                    # Send to current pane
codeterm send-text "command" --pane PANE_ID     # Send to specific pane
codeterm get-output                             # Read current pane output
codeterm get-output --pane PANE_ID              # Read specific pane output
```

## Workspace Management
```
codeterm workspace list                         # List workspaces
codeterm workspace current                      # Get current workspace ID
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
codeterm pane split
```

### Three-panel
```bash
codeterm pane split
codeterm pane split --id $(codeterm pane current --last) --direction down
```

## Agent Hierarchy

CodeTerm supports a 3-level hierarchy: Conductor -> Worker -> SubWorker (max depth 3).

```
codeterm agent list                            # List all agent panes
codeterm agent hierarchy                       # Show full hierarchy tree
codeterm agent set-conductor [--pane ID]       # Mark as conductor (depth 1)
codeterm agent link --parent ID [--pane ID]    # Link as worker under parent
codeterm agent unlink [--pane ID]              # Clear role
codeterm agent children [--pane ID]            # List children
codeterm agent allow-spawn [--pane ID]         # Toggle can_spawn
codeterm agent report MESSAGE [--pane ID]      # Send report to parent
codeterm agent reports [--pane ID]             # Read reports (as conductor)
codeterm agent status [--pane ID]              # Detailed status + role
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--agent-name NAME] [--args A1 A2]
                                               # One-call agent spawn (task uses -p flag, instant start)
```

### Orchestrator pattern (using agent spawn)
```bash
# 1. Mark self as conductor
codeterm agent set-conductor

# 2. Spawn workers (one-call: creates tab, sets role, launches agent, sends task via -p flag)
SELF=$(codeterm pane current)
codeterm agent spawn claude --title "Worker-1" --role Worker --parent $SELF --cwd /project --task "run tests"
codeterm agent spawn claude --title "Worker-2" --role Worker --parent $SELF --cwd /project --task "lint code"

# 3. Spawn with a specific agent definition from .claude/agents/
codeterm agent spawn claude --title "Architect" --role Worker --parent $SELF --cwd /project --agent-name architect --task "review architecture"

# 4. Read reports from workers
codeterm agent reports
```

### Orchestrator pattern (batch API — REST and Tauri IPC)
```bash
# Spawn entire hierarchy in one call (tasks use -p flag, instant start)
# Available via POST /api/batch (REST) or mux_batch Tauri command — identical semantics
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Conductor", "can_spawn": true, "cwd": "C:/project"}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_pane_id": "$boss", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_pane_id": "$boss", "task": "run tests"}},
      {"method": "notify", "params": {"title": "Ready", "body": "Hierarchy spawned"}}
    ]
  }'
```

Batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`.
Use `$id` to reference results from earlier commands (resolves to pane_id).

### Orchestrator pattern (manual — split + link)
```bash
# 1. Mark self as conductor
codeterm agent set-conductor

# 2. Split and assign workers
codeterm pane split
WORKER=$(codeterm pane current --last)
codeterm agent link --parent $(codeterm pane current) --pane $WORKER

# 3. Send work to worker pane
codeterm send-text "npm test" --pane $WORKER

# 4. Worker reports back
codeterm agent report "tests passed" --pane $WORKER

# 5. Conductor reads reports
codeterm agent reports
```

## Multi-agent Workflow
```bash
codeterm workspace create "frontend"
codeterm workspace create "backend"
# Each workspace gets its own Claude Code instance
```

## Session Persistence

Sessions auto-save every 30s. CWD is tracked via OSC 7 and restored per pane. Claude Code sessions are detected and auto-resumed with `--resume`.

## Themes

CodeTerm supports custom themes. Create a TOML file with 20 colors and apply it.

```bash
# List all themes (builtin + custom)
codeterm theme list

# Apply a theme
codeterm theme apply dracula

# Show custom themes directory
codeterm theme dir

# Save a custom theme from file
codeterm theme save my-theme /tmp/my-theme.toml

# Delete a custom theme
codeterm theme delete my-theme
```

### Creating a custom theme

Write a `.toml` file with all 20 color fields, save it, then apply:

```bash
cat > /tmp/my-theme.toml << 'EOF'
name = "My Custom Theme"
background = "#1a1b26"
foreground = "#c0caf5"
cursor = "#c0caf5"
selection = "#33467c"
black = "#15161e"
red = "#f7768e"
green = "#9ece6a"
yellow = "#e0af68"
blue = "#7aa2f7"
magenta = "#bb9af7"
cyan = "#7dcfff"
white = "#a9b1d6"
bright_black = "#414868"
bright_red = "#f7768e"
bright_green = "#9ece6a"
bright_yellow = "#e0af68"
bright_blue = "#7aa2f7"
bright_magenta = "#bb9af7"
bright_cyan = "#7dcfff"
bright_white = "#c0caf5"
EOF

codeterm theme save my-theme /tmp/my-theme.toml --apply
```

The UI colors (borders, sidebar, surfaces) are automatically derived from the background color. Light themes (bright background) get darkened borders; dark themes get lightened ones.

## Screenshots

Capture the CodeTerm window as JPEG. Works even when the window is behind other windows. Images are JPEG compressed (~200-400KB).

```
codeterm screenshot                            # Print JSON: {data_url, width, height, size_kb}
codeterm screenshot --output screenshot.jpg    # Save JPEG to file
```

REST API:
- `GET /api/screenshot` — JSON response with base64 data URL + dimensions
- `GET /api/screenshot?format=jpeg` — raw JPEG bytes

**For debugging:** save to file, then use a subagent to read and analyze the image (avoids context pollution with large base64 data).

```bash
codeterm screenshot --output /tmp/debug.jpg
# Then delegate image analysis to a subagent
```

## System
```
codeterm identify        # Version & features
codeterm capabilities    # All API methods
```

<!-- revision: 9 -->
