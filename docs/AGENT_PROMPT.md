You have access to a CodeTerm terminal multiplexer.

Core API:
- GET  /api/health — check connection
- GET  /api/workspaces — list workspaces
- POST /api/workspaces {name} — create workspace
- POST /api/workspaces/:id/tabs — create tab
- GET  /api/panes — list panes
- POST /api/panes/:id/send {text?, enter?, batch?} — send text (fire & forget)
- POST /api/panes/:id/exec {command, timeout_ms?, strip_ansi?} — send + wait for output
- GET  /api/panes/:id/output?offset=N&strip_ansi=true — get output (cursor-based, returns {output, offset})
- GET  /api/panes/:id/screen — get visible terminal text (including TUI alternate buffer)
- GET  /api/panes/:id/output/stream — SSE real-time output stream
- POST /api/panes/:id/split {direction} — split pane
- POST /api/panes/:id/rename {title} — rename tab
- POST /api/panes/:id/resize {cols, rows} — resize terminal

Themes:
- GET  /api/themes — list all themes
- POST /api/themes {id, toml} — save custom theme
- DELETE /api/themes/:id — delete custom theme
- POST /api/themes/apply {name} — apply theme

Integration:
- POST /api/browser/open {url} — open canvas panel (localhost, data URLs, HTML visualizations)
- POST /api/notifications {title, body, level} — send notification
- POST /api/webhooks {url, events?, once?, pane_id?, headers?, payload_extra?} — register webhook for real-time events
- GET  /api/network — connection info (local IP, Tailscale IP)
Webhook events: agent.permission_request, agent.task_complete, agent.report

Environment variables (available in every CodeTerm PTY):
- `$CODETERM` — always `1` inside CodeTerm
- `$CODETERM_PANE_ID` — this pane's unique ID (8-char hex, e.g. `a3f1b2c7`). Use for `--pane` args and agent hierarchy. Prefer over `$(codeterm pane current)`.
- `$CODETERM_ROLE` — your role: `Orchestrator`, `Lead`, or `Worker` (empty if no role). Set at PTY spawn.
- `$CODETERM_PARENT_PANE_ID` — your parent's pane ID (empty if Orchestrator or no role). Set at PTY spawn.
- `$CODETERM_SOCKET_PATH` — socket path for CLI communication
- `$TERM_PROGRAM` — always `CodeTerm`

Pane fields:
- `id` — unique pane ID (8-char hex, e.g. `a3f1b2c7`)
- `cwd` — current working directory (tracked via invisible OSC 7 shell integration)
- `agent_type` — `"claude"` when Claude Code is detected in the pane
- `claude_session_id` — session ID for resume (see below)
- `role` — `"Orchestrator"`, `"Lead"`, `"Worker"`, or null
- `parent_pane_id` — parent pane ID in hierarchy, or null
- `can_spawn` — whether this pane can spawn children (Orchestrator/Lead: true, Worker: always false)
- `depth` — hierarchy depth (0=none, 1=Orchestrator, 2=Lead or Worker, 3=Worker under Lead)

Agent Hierarchy:
- GET  /api/panes/:id/status — discover your role, parent, children
- GET  /api/agents/hierarchy — full hierarchy tree
- POST /api/panes/:id/role {role, parent_pane_id?, can_spawn?} — set role
- DELETE /api/panes/:id/role — clear role
- GET  /api/panes/:id/children — list child panes
- POST /api/panes/:id/report {message} — send report to direct parent (cascade, auto-injected into parent's PTY)
- GET  /api/panes/:id/reports?clear=true — read reports from children (optionally clearing them)
- POST /api/panes/:id/can-spawn {can_spawn} — toggle spawning permission
- GET  /api/agents — list all panes with roles

Claude session resume: when Claude Code is detected in a pane, CodeTerm saves its session ID. On restart, CodeTerm automatically runs `claude --resume <session-id>` to restore the conversation.

Workflow: create workspace → create tab → exec commands → monitor output via offset polling or SSE stream → receive webhook events when agents need attention.

Orchestrator workflow:
- For fire-and-forget commands: use `/exec` with `strip_ansi: true` to run a command and get clean output.
- For interactive TUI (e.g. Claude Code): use `/screen` to read what's currently visible on the terminal, `/send` to type input.
  - `enter: true` (default) auto-appends `\r` (Enter keystroke). Set `enter: false` to disable.
  - `batch` sends sequential commands with delays: `[{text, enter, delay_ms}]`.
  - Prefer agent spawn over manual launch — it handles `-p` flag, `--dangerously-skip-permissions`, and `--agent` automatically.
- For async tasks: register a one-shot webhook with `once: true` and `pane_id` to get notified when a specific pane's task completes. The webhook fires once and auto-deletes — like a Promise. Prefer this over polling with timeout.

Agent spawn (one-call):
- POST /api/agents/spawn {agent?, agent_name?, title?, cwd?, role?, parent_pane_id?, can_spawn?, task?, args?, workspace_id?}
- Creates a **new tab** (not a split) in the parent's workspace, assigns role, launches agent, optionally sends task. All in one request.
- Returns immediately (non-blocking) with {pane_id, tab_id, pty_id, agent, title, role}. PTY readiness and agent launch happen in a background thread.
- The hierarchy (Orchestrator -> Lead -> Worker) is a logical chain, not visual splits.
- `agent` defaults to "claude". For Claude, `--dangerously-skip-permissions` is added automatically.
- `agent_name` maps to `.claude/agents/` files (e.g. `"architect"` adds `--agent architect`).
- `task` is baked into the launch command via `-p` flag — agent starts working immediately, no delay.

Batch commands (POST /api/batch or Tauri `mux_batch` command):
- Send multiple operations in one request. Commands execute sequentially.
- Available from both REST API (POST /api/batch) and Tauri IPC (`invoke("mux_batch", {commands})`), with identical semantics.
- Body: `{commands: [{method, params, id?}]}`
- Available methods: spawn, send, rename, close, role, notify, can-spawn, report
- $ref syntax: give a command an `id`, then reference it with `$id` in later commands (resolves to pane_id).
- Both entry points delegate to the unified ServiceContext, ensuring consistent PTY cleanup, event emission, and hierarchy validation.
- Example — spawn a conductor + 2 workers in one call:
  ```json
  POST /api/batch
  {
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
      {"method": "spawn", "id": "lead", "params": {"title": "Lead-1", "role": "Lead", "parent_pane_id": "$boss", "agent": "claude", "task": "coordinate team"}},
      {"method": "spawn", "params": {"title": "Worker-1", "role": "Worker", "parent_pane_id": "$lead", "agent": "claude", "task": "analyze code"}},
      {"method": "spawn", "params": {"title": "Worker-2", "role": "Worker", "parent_pane_id": "$boss", "agent": "claude"}},
      {"method": "notify", "params": {"title": "Ready", "body": "3 agents spawned"}}
    ]
  }
  ```

Orchestrator with hierarchy:
- Orchestrator (depth 1) creates Leads and Workers via POST /api/agents/spawn, POST /api/batch, or Tauri `mux_batch` command.
- Leads (depth 2) can spawn Workers. Workers CANNOT spawn anything (`can_spawn` always false).
- Workers CANNOT call set-conductor, unlink, or agent spawn (backend blocks).
- Leads get `can_spawn=true` by default.
- Use `$CODETERM_PANE_ID` for self-identification instead of `$(codeterm pane current)`.
- Check `$CODETERM_ROLE` on startup to know your role (Orchestrator, Lead, Worker, or empty).
- Reports go to direct parent (cascade), not to orchestrator: Worker -> Lead -> Orchestrator.
- Reports are auto-injected into parent's PTY as `[Report from Name] message\r` -- no polling needed.
- The `agent.report` webhook event fires when any agent sends a report.
- Use GET /api/agents/hierarchy to see the full tree at any time.
- Closing a pane cascade-closes all children (tabs + PTYs killed recursively).
- Clearing a role (DELETE /api/panes/:id/role) orphans children -- but panes with a parent CANNOT unlink.
- Agent spawns do NOT auto-switch active tab.
- `workspace create` creates an empty workspace (no auto-tab).

<!-- revision: 11 -->
