---
name: codeterm
description: Control CodeTerm terminal multiplexer — workspaces, panes, notifications, layouts, agent hierarchy. Use when user mentions splitting panes, creating workspaces, sending commands to other panes, visualizations, managing terminal sessions, spawning workers/agents, orchestrating parallel tasks, or setting up multi-pane layouts. Also use when user says "in terminal", "split", "new pane", "run X there", "conductor", "worker", or wants to delegate tasks to sub-agents.
version: 0.2.1
---

# CodeTerm — AI Terminal Control

You are running inside CodeTerm, an AI-first terminal multiplexer. CLI: `codeterm`

**When to use CodeTerm agents vs built-in agents:** If user says "in terminal", "in codeterm", "spawn in tab", "open a terminal for", "заспавни в терминале" — use `codeterm agent spawn` (visible tabs with hierarchy). Otherwise, use the standard built-in Agent tool for background work.

## Auto-name your tab on session start

**Immediately rename your tab** based on the user's first message/task:
```bash
codeterm pane rename "fix login bug"
```

## Terminal Management

Commands default to the **current focused pane** when no ID is given.

**Environment variables** (available in every CodeTerm pane):
- `$CODETERM_PANE_ID` — this pane's unique ID, 8-char hex (e.g. `a3f1b2c7`). Use for `--pane` args.
- `$CODETERM_ROLE` — your role: `Conductor`, `Worker`, or `SubWorker` (empty if no role)
- `$CODETERM_PARENT_PANE_ID` — your parent's pane ID (empty if you're a conductor or no role)
- `$CODETERM_SOCKET_PATH` — socket path for CLI communication
- `$CODETERM` — always `1` when running inside CodeTerm

**Important:** Always use `$CODETERM_PANE_ID` to reference your own pane — `pane current` returns the *focused* pane (which may be a different pane). When splitting, always pass `--id $CODETERM_PANE_ID` to split *your* pane. Use `pane current --last` to get the ID of the newly created pane after a split.

```
codeterm pane list                              # List panes
codeterm pane current                           # Get focused pane ID (may not be yours!)
codeterm pane current --last                    # Get last created pane ID
codeterm pane split --id $CODETERM_PANE_ID      # Split YOUR pane right
codeterm pane split --id $CODETERM_PANE_ID --direction down  # Split YOUR pane down
codeterm pane rename "title"                    # Rename your tab
codeterm pane rename "title" --pane PANE_ID     # Rename specific pane
codeterm pane focus PANE_ID                     # Focus pane
codeterm pane close PANE_ID                     # Close pane
codeterm send-text "command" --pane PANE_ID     # Send to specific pane
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

## Canvas Panel

Render HTML visualizations inside CodeTerm. For localhost apps, data URLs, and generated content.

```bash
# Write HTML to file, then open as canvas panel
cat > /tmp/viz.html << 'EOF'
<!DOCTYPE html>
<html><head><style>
body { background: #0d1117; color: #c9d1d9; font-family: sans-serif; padding: 20px; }
</style></head><body><h1>Hello</h1></body></html>
EOF
codeterm browser open file:///tmp/viz.html
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

### Side-by-side (split + run command)
```bash
codeterm pane split
NEW_PANE=$(codeterm pane current --last)
codeterm send-text "npm test" --pane $NEW_PANE
```

### Three-panel (editor left, two panes right)
```bash
# Split right → capture top-right pane
codeterm pane split
TOP_RIGHT=$(codeterm pane current --last)

# Split that pane down → capture bottom-right pane
codeterm pane split --id $TOP_RIGHT --direction down
BOT_RIGHT=$(codeterm pane current --last)

# Send commands to each pane
codeterm send-text "npm test -- --watch" --pane $TOP_RIGHT
codeterm send-text "tail -f /var/log/app.log" --pane $BOT_RIGHT
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
codeterm agent report "MESSAGE" [--pane ID]    # Report to parent (positional, NOT --message)
codeterm agent reports [--pane ID]             # Read reports from children
codeterm agent status [--pane ID]              # Detailed status + role
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--workspace WS_ID] [--args A1 A2]
                                               # One-call: create tab, set role, launch agent, send task (non-blocking)
```

### Orchestrator pattern (using agent spawn)
```bash
# 1. Mark self as conductor (uses $CODETERM_PANE_ID — your pane's ID, set automatically)
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# 2. Spawn workers — each gets its own tab in your workspace, linked in hierarchy chain
codeterm agent spawn claude --title "Worker-1" --role Worker --parent $CODETERM_PANE_ID --cwd /project --task "run tests"
codeterm agent spawn claude --title "Worker-2" --role Worker --parent $CODETERM_PANE_ID --cwd /project --task "lint code"

# 3. Spawn with a specific agent definition from .claude/agents/
codeterm agent spawn claude --title "Architect" --role Worker --parent $CODETERM_PANE_ID --cwd /project --agent-name architect --task "review architecture"

# 4. Allow a worker to spawn sub-workers (3-level depth)
codeterm agent allow-spawn --pane <WORKER_PANE_ID>
codeterm agent spawn claude --title "SubWorker" --role SubWorker --parent <WORKER_PANE_ID> --task "subtask"

# 5. Spawn agent in a different workspace
WS=$(codeterm workspace create "Other Project" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Agent" --role Conductor --workspace $WS --cwd /other/project --task "analyze code"

# 6. Read reports from workers (poll until you get them)
codeterm agent reports
```

**Role-aware behavior — check `$CODETERM_ROLE` on startup:**

If `$CODETERM_ROLE` is `Worker` or `SubWorker`:
- You are in a hierarchy under a parent. The backend BLOCKS you from calling `set-conductor` or `unlink`.
- Do NOT create extra workspaces or tabs. Do NOT `sleep` or poll. Do NOT use files to exchange data.
- If asked to delegate/spawn sub-tasks, use codeterm: `codeterm agent spawn claude --title "Sub-X" --role SubWorker --parent $CODETERM_PANE_ID --task "do X then report back with: codeterm agent report your-summary"`
- Do NOT use the built-in Agent tool or run sub-tasks inline — always spawn via codeterm for visibility.
- When done, report to parent: `codeterm agent report "your summary"` — message is a positional arg, NOT `--message`.
- Reports are push-based: when your sub-worker reports, it appears in your terminal as `[Report from Sub-X] ...`. No polling.

If `$CODETERM_ROLE` is empty (no role assigned):
- You can become a conductor: `codeterm agent set-conductor --pane $CODETERM_PANE_ID`
- Then spawn workers: `codeterm agent spawn claude --title "W" --role Worker --parent $CODETERM_PANE_ID --can-spawn --task "..."`

**Report syntax:** `codeterm agent report "your message here"` — message is the FIRST positional argument. Do NOT use `--message`, `--to`, or `--from` flags — they don't exist.

**Cross-workspace spawn:** When creating a workspace and spawning an agent there, you MUST pass `--workspace <WS_ID>` explicitly, otherwise the agent lands in your current workspace:
```bash
WS=$(codeterm workspace create "Name" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Agent" --workspace $WS --task "do something"
```

**Hierarchy rules:** Conductor (depth 1) -> Worker (depth 2) -> SubWorker (depth 3, max). Use `--role SubWorker` when spawning under a Worker. Workers need `allow-spawn` before they can spawn SubWorkers. Each agent gets its own tab; the hierarchy is a logical chain, not visual splits. IDs are 8-char hex (e.g. `a3f1b2c7`). `--agent-name` (for `.claude/agents/` files) is only available via REST API, not CLI.

### Orchestrator pattern (batch API — REST and Tauri IPC)
```bash
# Spawn entire hierarchy in one call
# Available via POST /api/batch (REST) or mux_batch Tauri command — identical semantics
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Conductor", "can_spawn": true, "cwd": "/path/to/project"}},
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
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# 2. Split and assign workers
codeterm pane split
WORKER=$(codeterm pane current --last)
codeterm agent link --parent $CODETERM_PANE_ID --pane $WORKER

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

## System
```
codeterm identify        # Version & features
codeterm capabilities    # All API methods
```

## Platform Notes

- **Paths:** Use forward slashes in commands. The CLI handles path normalization.
- **Temp files:** Use `/tmp/` on macOS/Linux, `$TEMP` or `/tmp/` on Windows (both work in CodeTerm).

<!-- revision: 12 -->
