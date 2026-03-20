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

Integration:
- POST /api/browser/open {url} — open browser panel
- POST /api/notifications {title, body, level} — send notification
- POST /api/webhooks {url, events?, once?, pane_id?, headers?, payload_extra?} — register webhook for real-time events
- GET  /api/network — connection info (local IP, Tailscale IP)
- GET  /api/screenshot — capture window screenshot

Webhook events: agent.permission_request, agent.task_complete

Workflow: create workspace → create tab → exec commands → monitor output via offset polling or SSE stream → receive webhook events when agents need attention.

Orchestrator workflow:
- For fire-and-forget commands: use `/exec` with `strip_ansi: true` to run a command and get clean output.
- For interactive TUI (e.g. Claude Code): use `/screen` to read what's currently visible on the terminal, `/send` to type input.
  - `enter: true` auto-appends Enter keystroke (no need to add `\r` manually).
  - `batch` sends sequential commands with delays: `[{text, enter, delay_ms}]`.
  - Example — launching Claude Code: `{"batch": [{"text": "claude", "enter": true}, {"text": "", "enter": true, "delay_ms": 8000}]}`
- For async tasks: register a one-shot webhook with `once: true` and `pane_id` to get notified when a specific pane's task completes. The webhook fires once and auto-deletes — like a Promise. Prefer this over polling with timeout.
