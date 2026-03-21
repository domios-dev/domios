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
- POST /api/browser/open {url} — open browser panel
- POST /api/notifications {title, body, level} — send notification
- POST /api/webhooks {url, events?, once?, pane_id?, headers?, payload_extra?} — register webhook for real-time events
- GET  /api/network — connection info (local IP, Tailscale IP)
Webhook events: agent.permission_request, agent.task_complete, agent.report

Pane fields:
- `cwd` — current working directory (tracked via OSC 7)
- `agent_type` — `"claude"` when Claude Code is detected in the pane
- `claude_session_id` — session ID for resume (see below)
- `role` — `"Conductor"`, `"Worker"`, `"SubWorker"`, or null
- `parent_pane_id` — parent pane ID in hierarchy, or null
- `can_spawn` — whether this pane can spawn children
- `depth` — hierarchy depth (0=none, 1=Conductor, 2=Worker, 3=SubWorker)

Agent Hierarchy:
- GET  /api/panes/:id/status — discover your role, parent, children
- GET  /api/agents/hierarchy — full hierarchy tree
- POST /api/panes/:id/role {role, parent_pane_id?, can_spawn?} — set role
- DELETE /api/panes/:id/role — clear role
- GET  /api/panes/:id/children — list child panes
- POST /api/panes/:id/report {message} — worker sends report to conductor
- GET  /api/panes/:id/reports?clear=true — conductor reads (and optionally clears) reports
- POST /api/panes/:id/can-spawn {can_spawn} — toggle spawning permission
- GET  /api/agents — list all panes with roles

Claude session resume: when Claude Code is detected in a pane, CodeTerm saves its session ID. On restart, CodeTerm automatically runs `claude --resume <session-id>` to restore the conversation.

Workflow: create workspace → create tab → exec commands → monitor output via offset polling or SSE stream → receive webhook events when agents need attention.

Orchestrator workflow:
- For fire-and-forget commands: use `/exec` with `strip_ansi: true` to run a command and get clean output.
- For interactive TUI (e.g. Claude Code): use `/screen` to read what's currently visible on the terminal, `/send` to type input.
  - `enter: true` auto-appends Enter keystroke (no need to add `\r` manually).
  - `batch` sends sequential commands with delays: `[{text, enter, delay_ms}]`.
  - Example — launching Claude Code: `{"batch": [{"text": "claude", "enter": true}, {"text": "", "enter": true, "delay_ms": 8000}]}`
- For async tasks: register a one-shot webhook with `once: true` and `pane_id` to get notified when a specific pane's task completes. The webhook fires once and auto-deletes — like a Promise. Prefer this over polling with timeout.

Orchestrator with hierarchy:
- Conductor (depth 1) creates workers by splitting panes and assigning roles via POST /api/panes/:id/role.
- Workers (depth 2) can create sub-workers (depth 3) if `can_spawn` is true. Max depth is 3 levels.
- Workers report progress to their conductor via POST /api/panes/:id/report {message}.
- Conductor polls reports via GET /api/panes/:id/reports?clear=true.
- The `agent.report` webhook event fires when any worker sends a report.
- Use GET /api/agents/hierarchy to see the full tree at any time.
- Clearing a role (DELETE /api/panes/:id/role) orphans children — they keep their role but lose parent linkage.

<!-- revision: 5 -->
