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

codeterm theme save my-theme /tmp/my-theme.toml
codeterm theme apply my-theme
```

The UI colors (borders, sidebar, surfaces) are automatically derived from the background color. Light themes (bright background) get darkened borders; dark themes get lightened ones.

## System
```
codeterm identify        # Version & features
codeterm capabilities    # All API methods
```

<!-- revision: 3 -->
