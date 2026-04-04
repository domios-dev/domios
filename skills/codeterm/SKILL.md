---
name: codeterm
description: Control CodeTerm terminal multiplexer — workspaces, panes, notifications, layouts, agent hierarchy, cron jobs. Use when user mentions splitting panes, creating workspaces, sending commands to other panes, visualizations, managing terminal sessions, spawning workers/agents, orchestrating parallel tasks, setting up multi-pane layouts, or scheduling recurring tasks. Also use when user says "in terminal", "split", "new pane", "run X there", "orchestrator", "conductor", "lead", "worker", "cron", "schedule", "every hour", "recurring", or wants to delegate tasks to sub-agents.
version: 0.5.0
---

# CodeTerm — AI Terminal Control

You are running inside CodeTerm, an AI-first terminal multiplexer. CLI: `codeterm`

**Your scope:** This skill is the "spinal cord" — it teaches you how to use CodeTerm tools. You are NOT the General Agent (the machine-level brain). You are a project agent working in a specific workspace. If you need machine-level context (what other projects exist, system state, who's online in mesh), ask the local General Agent:
```bash
codeterm send-text "what projects are open right now?" --pane general
```
The General Agent's response arrives in your terminal. Use this when you don't know something about the environment rather than guessing.

**Language rule:** Agent-to-agent communication is always in English — regardless of the user's language. Task prompts, `--task` arguments, reports, cron tasks. Only reply to the user in their language.

**CRITICAL — Cron jobs / scheduled tasks / recurring tasks:**
Do NOT use the built-in CronCreate/CronList tools. CodeTerm has its own cron system. Always use `codeterm cron` commands:
```bash
# Create a cron job (schedule = 5-field: min hour dom mon dow)
codeterm cron create "Job Name" --schedule "0 * * * *" --task "what to do"

# Create a oneshot job (fires once, then auto-deletes)
codeterm cron create "Quick Backup" --schedule "in 30m" --task "run backup" --once
codeterm cron create "Deploy Tonight" --schedule "2026-04-05T02:00:00Z" --task "deploy to prod" --once

# Examples:
codeterm cron create "Heartbeat" --schedule "0 * * * *" --task "Say hello to the user"
codeterm cron create "Build Check" --schedule "*/30 * * * *" --task "Run cargo check and report"
codeterm cron create "Daily Review" --schedule "0 9 * * *" --task "Review TODOs in codebase" --cwd /path/to/project

# Manage
codeterm cron list                    # List all cron jobs
codeterm cron enable ID               # Enable
codeterm cron disable ID              # Disable
codeterm cron trigger ID              # Fire now
codeterm cron delete ID               # Delete
```
The cron spawns a Claude agent on schedule. The agent MUST call `codeterm cron complete --run-id UUID --summary "result"` when done (this instruction is auto-appended to the task prompt). Completing a run automatically closes the tab (run panes are ephemeral). Crons survive window close (app stays in tray). Successful spawns do not generate notifications — only errors do.

**CRITICAL — "separate workspace" / "another workspace" in user request:**
If the request mentions a separate/another/new/different workspace — run ONLY these 2 commands and stop:
```bash
WS=$(codeterm workspace create "Name" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn --task "You are the orchestrator. [full task]. Set yourself as conductor, spawn leads/workers, collect reports, send notification when done."
```
Do NOT call set-conductor on yourself. Do NOT spawn leads. Do NOT rename yourself to Orchestrator. Just 2 commands.

**When to use CodeTerm agents vs built-in agents:** If user says "in terminal", "in codeterm", "spawn in tab" — use `codeterm agent spawn`. Otherwise, use built-in Agent tool.

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

**Important:** Always use `$CODETERM_PANE_ID` to reference your own pane — `pane current` returns the *focused* pane (which may be a different pane).

```
codeterm pane list                              # List panes
codeterm pane current                           # Get focused pane ID (may not be yours!)
codeterm pane current --last                    # Get last created pane ID
codeterm pane split --id $CODETERM_PANE_ID      # Split pane right (only when user asks)
codeterm pane split --id $CODETERM_PANE_ID --direction down  # Split pane down
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
codeterm agent compact [--pane ID]             # Send /compact to Claude (compacts context)
codeterm agent model <sonnet|haiku|opus> [--pane ID]  # Switch Claude model (sends /model <name>)
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--workspace WS_ID] [--args A1 A2]
                                               # One-call: create tab (no auto-switch), set role, launch agent, send task (non-blocking)
                                               # Role: Orchestrator, Lead, Worker (accepts Conductor/SubWorker as aliases)
```

### "in a separate workspace" / "in another workspace" → delegate

If the user's request contains "separate workspace", "another workspace", "new workspace", "different workspace" — you MUST delegate. Run exactly these 2 commands and STOP:

```bash
WS=$(codeterm workspace create "Name" --json | python3 -c "import json,sys;print(json.load(sys.stdin)['id'])")
codeterm agent spawn claude --title "Orchestrator" --role Orchestrator --workspace $WS --can-spawn --task "[copy user's full request here, prefixed with: You are the orchestrator.]"
```

That's it. 2 commands. Do NOT call set-conductor. Do NOT spawn leads. Do NOT rename your pane to Orchestrator. The spawned agent does everything.

### Direct orchestration (no separate workspace mentioned)
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

## Mesh Networking

CodeTerm instances form a mesh — discover, pair, and communicate across machines.

**Any command works on a remote peer — add `--mesh TARGET` (name or ID):**
```bash
codeterm mesh status                           # Your identity, peers, access levels
codeterm mesh peers                            # Paired + discovered peers
codeterm pane list --mesh PEER                 # Remote panes
codeterm send-text "cmd" --pane ID --mesh PEER # Send to remote pane
codeterm get-output --pane ID --mesh PEER      # Read remote output
codeterm agent spawn claude --task "build" --mesh PEER  # Spawn remotely
codeterm workspace create "CI" --mesh PEER     # Create workspace remotely
```

**Flow:** `mesh peers` → find paired peer → `pane list --mesh PEER` → interact. Only paired peers accept commands (flat trust model: paired = trusted). AccessLevel (Full/ReadOnly/AgentIsolated) is per-peer, per-machine.

**Mesh message prefixes:** Incoming messages from mesh peers arrive as:
```
[Mesh: BLACKSTAR pane=a3f1b2c7] can you run the tests?
```
Reply to the source pane on the source peer:
```bash
codeterm send-text "tests pass" --pane a3f1b2c7 --mesh BLACKSTAR
```
To message a peer's General Agent: `--pane general --mesh PEER`. Include "report back" if you need a response.

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

## Cron Jobs

Schedule recurring or one-time tasks. Uses 5-field cron expressions (`min hour dom mon dow`) for recurring jobs, or relative/absolute time for oneshot jobs.

```bash
codeterm cron list [--workspace ID]            # List all cron jobs
codeterm cron create "name" --schedule "0 9 * * *" --task "Review TODOs" [--cwd /path] [--description "desc"] [--once]
                                               # --once: oneshot job (schedule accepts "in 5m", "in 1h30m", ISO8601, unix ms)
codeterm cron edit ID [--name "new"] [--schedule "..."] [--task "..."]
codeterm cron delete ID
codeterm cron enable ID
codeterm cron disable ID
codeterm cron trigger ID                       # Manual trigger
codeterm cron history ID                       # Run history
codeterm cron complete --run-id UUID --summary "what happened"
                                               # Mark a cron run as complete (called by spawned agent)
```

Cron runs spawn Claude agents on schedule. The spawned agent calls `cron complete` when done, which automatically closes the run's tab (run panes are ephemeral). Logs are saved to `~/.codeterm/crons/<job_id>/<run_id>.log` with SUMMARY and RAW OUTPUT sections. The sidebar shows a cron tree with runs and next scheduled time. Click a finished run to open the log viewer. The app hides to system tray on close so crons keep running. Successful spawns do not generate notifications — only errors do.

Deleting a cron job cascades: kills all running panes/PTYs, closes their tabs, and removes the log directory (`~/.codeterm/crons/<job_id>/`).

**IMPORTANT:** To modify a cron job, use `codeterm cron edit ID --name/--schedule/--task`. Do NOT delete and recreate — that loses run history and changes the ID. The `edit` command updates in-place.

**Workspace:** `cron create` uses `$CODETERM_WORKSPACE_ID` by default (your current workspace). Override with `--workspace ID` if needed.

Examples: `*/30 * * * *` (every 30 min), `0 9 * * *` (daily 9am), `0 9 * * 1` (Mondays 9am).

**Oneshot jobs:** Add `--once` to create a one-time task. The schedule accepts relative time (`"in 5m"`, `"in 1h30m"`, `"in 2d"`), ISO 8601 (`"2026-04-05T14:30:00Z"`), or unix milliseconds. Oneshot jobs fire once, then are disabled and auto-deleted on completion. `codeterm cron list` shows `[oneshot]` for these jobs.

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, splits, settings, and theme switching.

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

## Integrations

### Telegram Bridge
Connect a Telegram bot to the General Agent. CodeTerm handles inbound routing — the agent sends replies via CLI.

**Setup:**
1. Create a bot via @BotFather on Telegram, get the token
2. Save token as PLAIN TEXT (just the token, nothing else) to `~/.codeterm/secrets/telegram.token`:
   ```bash
   mkdir -p ~/.codeterm/secrets
   echo -n "YOUR_BOT_TOKEN_HERE" > ~/.codeterm/secrets/telegram.token
   ```
   IMPORTANT: The file must contain ONLY the token string — no markdown, no headers, no extra text.
3. Enable: `codeterm config set integrations.telegram_enabled true`
4. Optionally restrict users: `codeterm config set integrations.telegram_allowed_users '["186809246"]'`
5. Restart CodeTerm (hot-reload not yet supported)

**How it works:**
- Inbound (Telegram → Agent): messages arrive as `[Telegram: user_id] text` in the PTY. Rapid messages within 300ms are debounced into one delivery. Typing indicator is sent automatically.
- Outbound (Agent → Telegram): use `codeterm bridge send` CLI command. Supports HTML formatting (falls back to plain text). Works from **any pane**, not just the General Agent pane.
- `bridge send` (without `--append`) always sends a **new** message — no auto-edit.
- `--append` concatenates text to the previous message with an automatic `\n\n` paragraph break (agent does not need to add literal `\n\n`) and edits in-place within a 60-second window. After 60s, sends new.
- UTF-8 safe: messages are chunked safely for Cyrillic, emoji, and other multi-byte characters.
- Resilience: if Telegram polling dies, the bridge auto-restarts after 5 seconds.

**Sending messages:**
```bash
# Send a NEW message (always new, no auto-edit)
codeterm bridge send --to USER_ID --service telegram "Your reply here"

# Append to previous message (auto-inserts paragraph break, edits in-place within 60s)
codeterm bridge send --to USER_ID --service telegram --append "...more text"
```

**Sending media:**
```bash
# Photo with caption
codeterm bridge send --to USER_ID --type photo --file /path/to/image.png --caption "Check this out"

# Document with custom filename
codeterm bridge send --to USER_ID --type document --file /path/to/report.pdf --filename "Q1-report.pdf"

# Voice / Video / Animation (GIF) / Sticker
codeterm bridge send --to USER_ID --type voice --file /path/to/audio.ogg
codeterm bridge send --to USER_ID --type video --file /path/to/video.mp4
codeterm bridge send --to USER_ID --type animation --file /path/to/anim.gif
codeterm bridge send --to USER_ID --type sticker --file /path/to/sticker.webp
```

Supported `--type`: `text` (default), `photo`, `document`, `voice`, `animation`, `video`, `sticker`. Positional text is used as caption if `--caption` not set. Stickers have no caption.

**Settings UI:** Settings → Integrations → Telegram (toggle + allowed users)

### Config Management
Read/write CodeTerm config from the CLI:
```bash
codeterm config get integrations.telegram_enabled    # read a value
codeterm config set integrations.telegram_enabled true  # set a value
codeterm config get general_agent                    # read a section
```
Dot-notation for nested keys. Values are JSON (true, false, 42, "string", ["array"]).

## Platform Notes

- **Paths:** Use forward slashes in commands. The CLI handles path normalization.
- **Temp files:** Use `/tmp/` on macOS/Linux, `$TEMP` or `/tmp/` on Windows (both work in CodeTerm).

<!-- revision: 25 -->
