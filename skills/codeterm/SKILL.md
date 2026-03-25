---
name: codeterm
description: Control CodeTerm terminal multiplexer — workspaces, panes, notifications, layouts, agent hierarchy. Use when user mentions splitting panes, creating workspaces, sending commands to other panes, visualizations, managing terminal sessions, spawning workers/agents, orchestrating parallel tasks, or setting up multi-pane layouts. Also use when user says "in terminal", "split", "new pane", "run X there", "orchestrator", "conductor", "lead", "worker", or wants to delegate tasks to sub-agents.
version: 0.3.0
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
- `$CODETERM_ROLE` — your role: `Orchestrator`, `Lead`, or `Worker` (empty if no role)
- `$CODETERM_PARENT_PANE_ID` — your parent's pane ID (empty if you're an orchestrator or no role)
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
codeterm send-text "command" --pane PANE_ID     # Send to specific pane (--id alias for --pane)
codeterm get-output --pane PANE_ID              # Read specific pane output (--id alias for --pane)
```

## Workspace Management
```
codeterm workspace list                         # List workspaces
codeterm workspace current                      # Get current workspace ID
codeterm workspace create "Name"                # Create workspace (starts empty, no auto-tab)
codeterm workspace focus ID                     # Switch workspace
codeterm workspace rename ID "New Name"         # Rename
codeterm workspace close ID                     # Delete
codeterm workspace move-tab TAB FROM TO             # Move tab between workspaces
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

CodeTerm supports a 3-level hierarchy: Orchestrator → Lead → Worker.
- **Orchestrator** — top-level, can spawn Leads and Workers
- **Lead** — spawned by Orchestrator, can spawn Workers
- **Worker** — executes tasks, reports back, CANNOT spawn anything

```
codeterm agent list                            # List all agent panes
codeterm agent hierarchy                       # Show full hierarchy tree
codeterm agent set-conductor [--pane ID]       # Mark as orchestrator (alias: set-orchestrator)
codeterm agent link --parent ID [--pane ID]    # Link as worker under parent
codeterm agent unlink [--pane ID]              # Clear role
codeterm agent children [--pane ID]            # List children
codeterm agent allow-spawn [--pane ID]         # Toggle can_spawn
codeterm agent report "MESSAGE" [--pane ID]    # Report to parent (positional, NOT --message)
codeterm agent reports [--pane ID]             # Read reports from children
codeterm agent status [--pane ID]              # Detailed status + role
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--workspace WS_ID] [--args A1 A2]
                                               # One-call: create tab (no auto-switch), set role, launch agent, send task (non-blocking)
                                               # Role: Orchestrator, Lead, Worker (accepts Conductor/SubWorker as aliases)
```

### Delegating vs doing it yourself

When the user says "spawn an orchestrator in a workspace", "put an orchestrator there", "create a workspace and run X" — they want you to **delegate**. The whole point is that the orchestrator runs independently in its own workspace while you stay free. Think of it like hiring someone: you don't become the contractor, you hire one.

**Delegation pattern** (2 commands, then you're done):
```bash
WS=$(codeterm workspace create "Project" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn --task "You are the orchestrator. [paste the full task here]. Set yourself as conductor, spawn leads/workers, collect reports, send notification when done."
```
After these 2 commands, you're **done**. Do not call set-conductor. Do not spawn leads. Do not touch the Demo workspace. The spawned orchestrator handles everything autonomously.

When the user says "be the orchestrator", "set yourself up", "spawn workers for me" — then you DO it yourself:

### Self-orchestration pattern (when YOU are the orchestrator)
```bash
# 1. Mark self as orchestrator
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# 2. Spawn leads (they can spawn workers under them)
codeterm agent spawn claude --title "Team-A" --role Lead --parent $CODETERM_PANE_ID --task "coordinate team A"

# 3. Spawn workers directly (they execute and report, cannot spawn)
codeterm agent spawn claude --title "Scout" --role Worker --parent $CODETERM_PANE_ID --task "list files and report"

# 4. Reports arrive automatically — no polling needed
```

**Role-aware behavior — check `$CODETERM_ROLE` on startup:**

If `$CODETERM_ROLE` is `Lead`:
- You are under an Orchestrator. Do NOT call `set-conductor` or `unlink` (backend blocks it).
- You CAN spawn Workers: `codeterm agent spawn claude --title "W" --role Worker --parent $CODETERM_PANE_ID --task "do X then run: codeterm agent report 'your result summary'"`
- IMPORTANT: Always include `codeterm agent report '...'` in the worker's --task so it knows how to report back.
- After spawning workers, continue your own work or spawn more. Do NOT block waiting for reports — no sleep, no polling loops, no `codeterm agent reports` in a loop. Reports arrive automatically as `[Report from ...]` messages in your terminal. When you see one, react to it.
- Do NOT create extra workspaces or tabs. Do NOT use files to exchange data.
- When all expected reports have arrived, run: `codeterm agent report "your summary"`

If `$CODETERM_ROLE` is `Worker`:
- You execute tasks and report. You CANNOT spawn anything.
- Do NOT call `set-conductor`, `unlink`, `agent spawn`, or create tabs.
- Do NOT use SendMessage or built-in Agent tools — use `codeterm agent report` CLI only.
- Do NOT sleep. Execute the task, then immediately: `codeterm agent report "your summary"`

If `$CODETERM_ROLE` is empty (Orchestrator):
- You can become orchestrator: `codeterm agent set-conductor --pane $CODETERM_PANE_ID`
- Spawn leads/workers with `codeterm agent spawn`
- After spawning, stay operational — continue your work. Do NOT block with sleep or polling. Reports arrive automatically as `[Report from ...]` messages. React when you see them.

**Report syntax:** `codeterm agent report "message"` — message is the FIRST positional argument. Do NOT use `--message`, `--to`, or `--from` flags.

**Hierarchy:** Orchestrator (depth 1) → Lead (depth 2) → Worker (depth 3). Leads can spawn Workers. Workers cannot spawn. IDs are 8-char hex.

### Orchestrator pattern (batch API — REST and Tauri IPC)
```bash
# Spawn entire hierarchy in one call
# Available via POST /api/batch (REST) or mux_batch Tauri command — identical semantics
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "can_spawn": true, "cwd": "/path/to/project"}},
      {"method": "spawn", "id": "lead", "params": {"title": "Lead-A", "role": "Lead", "parent_pane_id": "$boss", "task": "coordinate team"}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_pane_id": "$lead", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_pane_id": "$boss", "task": "run tests"}},
      {"method": "notify", "params": {"title": "Ready", "body": "Hierarchy spawned"}}
    ]
  }'
```

Batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`.
Use `$id` to reference results from earlier commands (resolves to pane_id).

### Orchestrator pattern (manual -- split + link)
```bash
# 1. Mark self as orchestrator
codeterm agent set-conductor --pane $CODETERM_PANE_ID

# 2. Split and assign workers
codeterm pane split
WORKER=$(codeterm pane current --last)
codeterm agent link --parent $CODETERM_PANE_ID --pane $WORKER

# 3. Send work to worker pane
codeterm send-text "npm test" --pane $WORKER

# 4. Reports arrive automatically as [Report from ...] in your PTY -- no polling needed
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

## Testing the orchestrator

When asked to test the CodeTerm orchestrator:
1. Run `bash tests/restart-clean.sh` — kills all, cleans session, restarts. Returns socket path.
2. Verify clean: 1 pane, no hierarchy
3. Spawn Boss with task, wait for notifications/reports
4. Check hierarchy (`codeterm agent hierarchy`), reports, pane outputs
5. If issues — read each pane's output, find errors, fix skill/code, repeat from step 1

## Platform Notes

- **Paths:** Use forward slashes in commands. The CLI handles path normalization.
- **Temp files:** Use `/tmp/` on macOS/Linux, `$TEMP` or `/tmp/` on Windows (both work in CodeTerm).

<!-- revision: 13 -->
