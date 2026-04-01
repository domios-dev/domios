# CodeTerm API Reference

CodeTerm exposes three APIs for agent integration:
1. **REST API** — HTTP on port 7685 (configurable, binds to 0.0.0.0 by default)
2. **Socket API** — JSON-RPC v2 over named pipe (Windows) / Unix socket
3. **CLI** — `codeterm.exe` command-line tool

### Architecture: Unified Service Layer

All multiplexer operations (workspace/tab/pane management, agent spawn, batch commands, etc.) are implemented in a single **ServiceContext** (`src-tauri/src/service.rs`). Both REST API routes and Tauri IPC commands delegate to this shared service layer, which ensures consistent behavior — PTY cleanup, event emission, hierarchy validation — regardless of the entry point. This means every operation available via REST is also available internally via Tauri commands, with identical semantics.

## REST API

Base URL: `http://<host>:7685/api`

### Authentication
Optional Bearer token. Set in config:
```toml
[api]
host = "0.0.0.0"
port = 7685
token = "your-secret-token"
```
Then: `Authorization: Bearer your-secret-token`

### Endpoints

#### System
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/health` | — | Health check |
| GET | `/api/identify` | — | Version, platform, features |
| GET | `/api/network` | — | Local IP, Tailscale IP, API URLs |
| GET | `/api/screenshot` | `?format=json\|jpeg` | Capture window screenshot (see below) |


#### Workspaces
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/workspaces` | — | List all workspaces |
| POST | `/api/workspaces` | `{name}` | Create workspace |
| POST | `/api/workspaces/{id}/focus` | — | Switch to workspace |
| POST | `/api/workspaces/{id}/rename` | `{name}` | Rename workspace |
| POST | `/api/workspaces/{id}/close` | — | Delete workspace |
| POST | `/api/workspaces/{id}/tabs` | — | Create tab |
| POST | `/api/workspaces/{id}/tabs/{tid}/move` | `{to_workspace_id}` | Move tab between workspaces |

#### Panes
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/panes` | — | List all panes |
| POST | `/api/panes/{id}/send` | `{text?, enter?, batch?: [{text, enter?, delay_ms?}]}` | Send text to PTY (fire & forget) |
| POST | `/api/panes/{id}/exec` | `{command, timeout_ms?, strip_ansi?}` | Send command + wait for output |
| GET | `/api/panes/{id}/output` | `?offset=N&strip_ansi=true` | Get output buffer (cursor-based) |
| GET | `/api/panes/{id}/screen` | — | Get visible terminal text (alternate buffer included) |
| GET | `/api/panes/{id}/output/stream` | — | SSE real-time output stream |
| POST | `/api/panes/{id}/focus` | — | Focus pane |
| POST | `/api/panes/{id}/close` | — | Close pane + kill PTY |
| POST | `/api/panes/{id}/split` | `{direction: "right"\|"down"}` | Split pane |
| POST | `/api/panes/{id}/rename` | `{title}` | Rename pane/tab (pinned) |
| POST | `/api/panes/{id}/resize` | `{cols, rows}` | Resize terminal |
| POST | `/api/panes/{id}/hook/permission` | `{question?, options?: [{label, value}]}` | Notify that agent is showing a permission prompt (see below) |

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/panes/{id}/role` | `{role, parent_pane_id?, can_spawn?}` | Set pane role (`Orchestrator`, `Lead`, `Worker`). Accepts `Conductor`/`SubWorker` as backward-compat aliases. |
| DELETE | `/api/panes/{id}/role` | — | Clear role (orphans children) |
| GET | `/api/panes/{id}/status` | — | Pane status + role + children info |
| GET | `/api/panes/{id}/children` | — | List child panes |
| POST | `/api/panes/{id}/report` | `{message}` | Send report to direct parent (cascade, not to Orchestrator). Auto-injected into parent's PTY. |
| GET | `/api/panes/{id}/reports` | `?clear=true` | Read reports from children (optionally clearing them) |
| POST | `/api/panes/{id}/can-spawn` | `{can_spawn: bool}` | Toggle whether pane can spawn children |
| GET | `/api/agents` | — | List all panes that have roles |
| GET | `/api/agents/hierarchy` | — | Full hierarchy tree |
| POST | `/api/agents/spawn` | `{agent?, agent_name?, title?, cwd?, role?, parent_pane_id?, can_spawn?, task?, args?, workspace_id?}` | One-call agent spawn (see below) |

#### Batch
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/batch` | `{commands: [{method, params, id?}]}` | Execute multiple commands sequentially (see below) |

### Pane Fields

Fields returned by `GET /api/panes` for each pane:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique pane ID (8-char hex, e.g. `a3f1b2c7`) |
| `pty_id` | `string` | Associated PTY process ID |
| `pane_type` | `string` | `"Terminal"` or `"Browser"` (canvas for visualizations) |
| `title` | `string` | Display title (user-pinned or auto-detected) |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `url` | `string?` | URL for canvas panes (localhost, data URLs) |
| `agent_type` | `string?` | Agent running in pane, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |
| `role` | `string?` | `"Orchestrator"`, `"Lead"`, `"Worker"`, or `null` |
| `parent_pane_id` | `string?` | Parent pane ID in hierarchy, or `null` |
| `can_spawn` | `bool` | Whether this pane can spawn children. Orchestrator: true (default). Lead: true (default). Worker: always false. |
| `connected_services` | `string[]` | External services connected to this pane (e.g. `["telegram"]`). Shown as icons in sidebar. |
| `depth` | `number` | Hierarchy depth: 0 (no role), 1 (Orchestrator), 2 (Lead or Worker under Orchestrator), 3 (Worker under Lead) |

#### Permission Hook (POST /api/panes/{id}/hook/permission)

Called by Claude Code hooks (or any agent integration) to notify CodeTerm that the agent is showing a permission prompt. This is a faster path than PTY auto-detection and bypasses the cooldown period.

**Two modes:**

1. **Explicit prompt** -- provide `question` and `options` in the body. CodeTerm uses them directly.
2. **Auto-parse** -- send an empty body `{}`. CodeTerm reads the terminal screen content and runs structural fingerprint detection to extract the prompt.

```bash
# Auto-parse (most common — Claude hooks just ping the endpoint)
curl -X POST "http://host:7685/api/panes/$PANE/hook/permission" \
  -H "Content-Type: application/json" -d '{}'

# Explicit prompt
curl -X POST "http://host:7685/api/panes/$PANE/hook/permission" \
  -H "Content-Type: application/json" \
  -d '{"question":"Allow file write?","options":[{"label":"Yes","value":"1"},{"label":"No","value":"3"}]}'
```

Response: `{"detected": true}` or `{"detected": false}`. Even when `detected: false`, the hook still fires a `agent.permission_request` webhook event (the hook firing itself signals the agent is waiting).

#### Stop Hook (POST /api/panes/{id}/hook/stop)

Called by Claude Code hooks (Stop matcher) to notify CodeTerm that the agent has completed its task. Triggers `agent.task_complete` webhook event and updates pane status. For protected panes (e.g. General Agent), the stop hook does NOT clear `agent_type` — the pane retains its agent identity.

```bash
curl -X POST "http://host:7685/api/panes/$PANE/hook/stop" \
  -H "Content-Type: application/json" -d '{}'
```

#### Canvas (Visualizations)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/browser/open` | `{url}` | Open canvas panel (localhost, data URLs, HTML) |
| GET | `/api/serve-file` | `?path=...` | Serve local file for canvas |

#### Notifications
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/notifications` | `?unread=true` | List notifications |
| POST | `/api/notifications` | `{title, body?, level?}` | Create notification (+ OS toast if unfocused) |
| POST | `/api/notifications/clear` | — | Clear all |

#### Webhooks
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/webhooks` | — | List registered webhooks |
| POST | `/api/webhooks` | `{url, events?, once?, pane_id?, headers?, payload_extra?}` | Register webhook |
| DELETE | `/api/webhooks/{id}` | — | Unregister webhook |

Webhook events: `agent.permission_request`, `agent.task_complete`, `agent.report`

#### Themes
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/themes` | — | List all themes (builtin + custom) |
| POST | `/api/themes` | `{id, toml}` | Save a custom theme |
| DELETE | `/api/themes/{id}` | — | Delete a custom theme |
| POST | `/api/themes/apply` | `{name}` | Apply a theme by name |

Custom themes are stored as `.toml` files in `%APPDATA%/codeterm/themes/` (Windows) or `~/.codeterm/themes/` (macOS/Linux).

#### Bridge (Telegram / External Services)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/bridge/send` | `{text, chat_id?, service?, append?, type?, file?, caption?, filename?}` | Send message or media to external service (Telegram) |

**POST /api/bridge/send** — sends an outbound message or media through the bridge. Parameters:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `text` | `string` | *(required for text)* | Message text (HTML parse mode). Required when `type` is `text` or omitted. Used as caption fallback for media types if `caption` is not set. |
| `chat_id` | `number?` | last inbound | Telegram user/chat ID. Defaults to the last inbound sender. |
| `service` | `string?` | `"telegram"` | Target service (currently only `telegram`) |
| `append` | `bool` | `false` | Append text to previous message (edits via `editMessageText`) instead of sending new |
| `type` | `string?` | `"text"` | Message type: `text`, `photo`, `document`, `voice`, `animation`, `video`, `sticker` |
| `file` | `string?` | — | Absolute path to the file to send. Required for all media types. |
| `caption` | `string?` | — | Caption for media messages. If not set, positional `text` is used as caption. Stickers do not support captions. |
| `filename` | `string?` | — | Custom filename for `document` type (overrides the original filename). |

**Behavior:** `bridge send` (without `--append`) always sends a **new** message — no auto-edit. With `--append`, text is concatenated to the previous message (with an automatic `\n\n` paragraph break) and edited in-place, provided the previous message was sent within the last 60 seconds. If the 60-second window has expired, `--append` sends a new message instead.

**Typing indicator:** On every inbound Telegram message, CodeTerm sends a `sendChatAction("typing")` to show the typing bubble.

**Inbound debounce:** Rapid-fire Telegram messages within a 300ms window are merged into a single delivery to the agent PTY.

**UTF-8 safe chunking:** Messages exceeding Telegram's 4096-character limit are split into chunks. The chunker uses `floor_char_boundary()` to avoid slicing multi-byte UTF-8 characters (Cyrillic, emoji, CJK), preventing panics on non-ASCII text.

**Any-pane sending:** `bridge send` works from any pane, not just the General Agent pane. The CLI routes through the socket API without pane restrictions.

**Service resilience:** If the Telegram polling service dies (network error, API failure), the bridge manager automatically restarts it after a 5-second backoff. The manager never exits — it loops indefinitely, logging panics instead of dying silently.

Returns `{"ok": true}` on success. Returns `503 Service Unavailable` if bridge is not active.

#### Setup (Skills, Rules, Hooks)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/setup/skills` | — | List available skills and install status |
| POST | `/api/setup/skills/{id}/install` | — | Install a skill |
| POST | `/api/setup/skills/{id}/uninstall` | — | Uninstall a skill |
| GET | `/api/setup/rules` | — | Check if project rules are installed |
| POST | `/api/setup/rules/install` | — | Install project rules |
| GET | `/api/setup/hooks` | — | Check Claude hooks status |
| POST | `/api/setup/hooks/install` | — | Install Claude hooks |
| POST | `/api/setup/hooks/uninstall` | — | Uninstall Claude hooks |

#### Cron Jobs
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/crons` | `?workspace_id=ID` | List all cron jobs (optionally filter by workspace) |
| POST | `/api/crons` | `{name, schedule, task, workspace_id, agent?, cwd?, description?}` | Create cron job |
| GET | `/api/crons/:id` | — | Get cron job by ID |
| PUT | `/api/crons/:id` | `{name?, schedule?, task?, cwd?, description?, enabled?}` | Update cron job (prefer over delete+recreate) |
| DELETE | `/api/crons/:id` | — | Delete cron job |
| POST | `/api/crons/:id/enable` | — | Enable cron job |
| POST | `/api/crons/:id/disable` | — | Disable cron job |
| POST | `/api/crons/:id/trigger` | — | Manually trigger cron job |
| GET | `/api/crons/:id/history` | — | Get run history for cron job |

#### Configuration
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/config` | `?key=dot.path` | Get full config or a single key (dot-notation) |
| PATCH | `/api/config` | `{key, value}` or partial merge | Set a config value by dot-notation key, or merge a partial config object |

**GET /api/config** — without `key`, returns the full `AppConfig` as JSON. With `?key=integrations.telegram_enabled`, returns `{"key": "...", "value": ...}`.

**PATCH /api/config** — two modes:
1. **Dot-notation key+value:** `{"key": "integrations.telegram_enabled", "value": true}` — sets a single key.
2. **Partial merge:** send top-level config sections as JSON — they are merged into the existing config.

The config is saved to disk and a `config-changed` Tauri event is emitted.

#### Updates
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/update/check` | `?force=true` | Check for available updates |
| POST | `/api/update/install` | — | Download and install pending update |

Schedule format: 5-field cron expressions (`min hour dom mon dow`). Examples: `*/30 * * * *` (every 30 min), `0 9 * * *` (daily 9am), `0 9 * * 1` (Mondays 9am).

Cron runs spawn Claude agents on schedule. When done, the agent calls `codeterm cron complete --run-id UUID` (uses the `cron.complete` socket method), which automatically closes the run's tab/pane (ephemeral). Logs are saved to `~/.codeterm/crons/<job_id>/<run_id>.log` with SUMMARY and RAW OUTPUT sections. The sidebar shows a cron tree with runs and next scheduled time. Click a finished run to open the log viewer. Successful spawns do not generate notifications — only errors do.

**Workspace resolution:** The CLI `cron create` command uses `$CODETERM_WORKSPACE_ID` by default (the agent's own workspace), falling back to the first workspace if the env var is not set. Override with `--workspace ID`.

**Updating cron jobs:** Use `PUT /api/crons/:id` (CLI: `codeterm cron edit ID`) to modify a cron job in-place. Do NOT delete and recreate -- that loses run history and changes the ID.

**Deleting a cron job** cascades: kills all running panes/PTYs for that job, closes their tabs, and removes the entire log directory (`~/.codeterm/crons/<job_id>/`).

**Tauri events:**
- `cron-state-changed` — emitted on any cron CRUD operation
- `cron-fired` — emitted when a cron fires, payload: `{cron_id, run_id, pane_id, tab_id}`

### Agent Spawn (POST /api/agents/spawn)

Creates a **new tab** in the parent's workspace, assigns a role, launches an agent (claude/aider/any), and optionally sends a task. All in one request. The response returns immediately (non-blocking) -- PTY readiness detection and agent launch happen in a background thread.

The hierarchy (Orchestrator -> Lead -> Worker) is a logical chain, not visual splits. Each agent gets its own tab. Agent spawns do not auto-switch the active tab -- the user stays on their current tab.

**Request body:**
```json
{
  "agent": "claude",           // agent command (default: "claude")
  "agent_name": "architect",   // maps to --agent flag (.claude/agents/ file)
  "title": "Worker-1",         // tab title (default: agent_name or agent command)
  "cwd": "C:/project",         // working directory
  "role": "Worker",            // Orchestrator, Lead, Worker (accepts Conductor/SubWorker as aliases)
  "parent_pane_id": "a3f1b2c7", // parent for Lead/Worker (8-char hex ID)
  "can_spawn": true,           // allow spawning children
  "task": "analyze the code",  // baked into launch via -p flag (instant start)
  "args": ["--verbose"],       // extra agent args
  "workspace_id": "b4e2c8d1"  // optional, defaults to active workspace
}
```

All fields are optional. `agent` defaults to `"claude"`. For Claude, `--dangerously-skip-permissions` is added automatically. When `task` is provided, it is passed via the `-p` flag so the agent starts working immediately (no delay). When `agent_name` is provided, it maps to `--agent <name>` which loads the agent definition from `.claude/agents/<name>.md`.

**Response** (returned immediately, before agent is fully started):
```json
{"pane_id": "a3f1b2c7", "tab_id": "d5e6f7a8", "pty_id": "c9d0e1f2", "agent": "claude", "title": "Worker-1", "role": "Worker"}
```

### Batch Commands (POST /api/batch)

Execute multiple operations in a single HTTP request. Commands run sequentially. Each command can have an `id` that other commands reference via `$id` (resolves to `pane_id` from that result).

**Request body:**
```json
{
  "commands": [
    {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
    {"method": "spawn", "id": "lead", "params": {"title": "Lead-1", "role": "Lead", "parent_pane_id": "$boss", "agent": "claude", "cwd": "C:/project", "task": "coordinate team"}},
    {"method": "spawn", "params": {"title": "Worker-1", "role": "Worker", "parent_pane_id": "$lead", "agent": "claude", "task": "analyze code"}},
    {"method": "spawn", "params": {"title": "Worker-2", "role": "Worker", "parent_pane_id": "$boss", "agent": "claude"}},
    {"method": "notify", "params": {"title": "All spawned", "body": "4 agents ready"}}
  ]
}
```

**Available methods:** `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`

**$ref syntax:** Any string value starting with `$` is replaced with the `pane_id` from the result of the command with that `id`. E.g. `"parent_pane_id": "$boss"` resolves to the pane_id returned by the "boss" spawn command.

**Response:**
```json
{"results": [{"pane_id": "...", "tab_id": "...", ...}, {"ok": true}, ...]}
```

On error, individual commands return `{"error": "message", "method": "..."}` but subsequent commands still execute.

**Batch method parameters:**

| Method | Required params | Optional params |
|--------|----------------|-----------------|
| `spawn` | — | `agent`, `agent_name`, `title`, `cwd`, `role`, `parent_pane_id`, `can_spawn`, `task`, `args` |
| `send` | `pane_id`, | `text`, `enter` (default: true) |
| `rename` | `pane_id`, `title` | — |
| `close` | `pane_id` | — |
| `role` | `pane_id`, `role` | `parent_pane_id`, `can_spawn` |
| `notify` | — | `title`, `body`, `level` |
| `can-spawn` | `pane_id` | `can_spawn` (default: true) |
| `report` | `pane_id`, `message` | — |

### Output Cursor Pattern

Use `offset` parameter for efficient polling without fetching the full buffer:
```bash
# First call — get all output
curl "http://host:7685/api/panes/$PANE/output"
# Response: {"output": "...", "offset": 1234}

# Subsequent calls — only new data
curl "http://host:7685/api/panes/$PANE/output?offset=1234"
# Response: {"output": "new stuff", "offset": 1500}
```

### ANSI Stripping

Add `strip_ansi=true` to get clean text without escape codes:
- `GET /api/panes/:id/output?strip_ansi=true`
- `POST /api/panes/:id/exec {"command": "...", "strip_ansi": true}`

### Exec Pattern

Send a command and wait for its output in one call:
```bash
curl -X POST "http://host:7685/api/panes/$PANE/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "echo hello", "timeout_ms": 5000}'
# Response: {"output": "hello\r\n...", "offset": 1600}
# If timeout: {"output": "...", "offset": 1600, "timeout": true}
```

### SSE Output Stream

Subscribe to real-time output via Server-Sent Events:
```bash
curl -N "http://host:7685/api/panes/$PANE/output/stream"
# data: {"output": "new text", "offset": 1234}
# data: {"output": "more text", "offset": 1300}
```

### Screenshot (GET /api/screenshot)

Captures the CodeTerm window as a JPEG image. Works even when the window is behind other windows.

**Platform implementation:**
- **Windows:** PrintWindow API (captures occluded windows)
- **macOS:** `screencapture -l` (captures by window ID)
- **Linux:** `xdotool` + `import` (X11 window capture)

**Query parameters:**

| Param | Values | Default | Description |
|-------|--------|---------|-------------|
| `format` | `json`, `jpeg` | `json` | Response format |

**format=json** (default) — returns JSON with base64 data URL and metadata:
```json
{
  "data_url": "data:image/jpeg;base64,/9j/4AAQ...",
  "width": 1920,
  "height": 1080,
  "size_kb": 287
}
```

**format=jpeg** — returns raw JPEG bytes with `Content-Type: image/jpeg`.

```bash
# Get screenshot as JSON (data URL + metadata)
curl http://host:7685/api/screenshot

# Save raw JPEG to file
curl http://host:7685/api/screenshot?format=jpeg -o screenshot.jpg
```

### Examples

```bash
# Health check
curl http://host:7685/api/health

# Create workspace + tab
WS=$(curl -s -X POST http://host:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"my-project"}' | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

# Get pane and run command
PANE=$(curl -s http://host:7685/api/panes | python3 -c "import sys,json;print(json.load(sys.stdin)[-1]['id'])")
curl -X POST "http://host:7685/api/panes/$PANE/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "npm test", "timeout_ms": 30000}'

# Send with batch (e.g. launch Claude Code)
curl -X POST "http://host:7685/api/panes/$PANE/send" \
  -H "Content-Type: application/json" \
  -d '{"batch": [{"text": "claude", "enter": true}, {"text": "", "enter": true, "delay_ms": 8000}]}'

# Register webhook for agent events
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/hook", "events": ["agent.permission_request"]}'

# One-shot webhook (fires once, then auto-deletes — like a Promise)
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/done", "events": ["agent.task_complete"], "once": true, "pane_id": "'"$PANE"'"}'

# Network info
curl http://host:7685/api/network
# {"local_ip":"192.168.1.50","tailscale_ip":"100.64.10.1","api_url":"http://192.168.1.50:7685"}
```

## Socket API (JSON-RPC v2)

### Connection
- **Windows**: Named pipe `\\.\pipe\codeterm-{PID}`
- **macOS/Linux**: Unix socket `/tmp/codeterm-{PID}.sock`
- Auto-discovery: `CODETERM_SOCKET_PATH` env var

### Protocol
Newline-delimited JSON:
```json
{"jsonrpc":"2.0","id":1,"method":"system.identify","params":null}
```

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `system.identify` | — | Version, platform, features |
| `system.capabilities` | — | List all methods |
| `workspace.list` | — | List workspaces |
| `workspace.create` | `{name}` | Create workspace |
| `workspace.focus` | `{workspace_id}` | Focus workspace |
| `workspace.rename` | `{workspace_id, name}` | Rename workspace |
| `workspace.close` | `{workspace_id}` | Close workspace |
| `workspace.move_tab` | `{tab_id, from_workspace_id, to_workspace_id}` | Move tab |
| `pane.list` | — | List panes |
| `pane.focus` | `{pane_id}` | Focus pane |
| `pane.close` | `{pane_id}` | Close pane |
| `pane.send_text` | `{pane_id, text, enter?}` | Send text to PTY (`enter` defaults to `true`, appends `\r`) |
| `pane.get_output` | `{pane_id, max_bytes?}` | Read terminal output |
| `pane.split` | `{pane_id, direction?}` | Split pane |
| `pane.rename` | `{pane_id, title}` | Rename pane (pinned) |
| `browser.open` | `{url}` | Open canvas panel (localhost, data URLs) |
| `notification.create` | `{title, body?, level?, pane_id?}` | Create notification |
| `notification.list` | `{unread_only?}` | List notifications |
| `notification.clear` | `{id?}` | Clear notifications |
| `agent.list` | — | List all panes with roles |
| `agent.hierarchy` | — | Full hierarchy tree |
| `agent.spawn` | `{agent?, title?, cwd?, role?, parent_pane_id?, can_spawn?, task?, args?, workspace_id?}` | Spawn agent (same as REST) |
| `agent.set_role` | `{pane_id, role, parent_pane_id?, can_spawn?}` | Set pane role |
| `agent.clear_role` | `{pane_id}` | Clear pane role |
| `agent.status` | `{pane_id}` | Get pane status + role |
| `agent.children` | `{pane_id}` | List child panes |
| `agent.link` | `{pane_id, parent_pane_id}` | Link pane under parent |
| `agent.report` | `{pane_id, message}` | Send report to direct parent |
| `agent.reports` | `{pane_id, clear?}` | Read reports from children |
| `cron.list` | `{workspace_id?}` | List cron jobs |
| `cron.get` | `{id}` | Get cron job by ID |
| `cron.create` | `{name, schedule, task, workspace_id, agent?, cwd?, description?}` | Create cron job |
| `cron.update` | `{id, name?, schedule?, task?, cwd?, description?, enabled?}` | Update cron job |
| `cron.delete` | `{id}` | Delete cron job |
| `cron.enable` | `{id}` | Enable cron job |
| `cron.disable` | `{id}` | Disable cron job |
| `cron.trigger` | `{id}` | Manually trigger cron job |
| `cron.history` | `{id}` | Get run history |
| `cron.complete` | `{run_id, summary?}` | Mark a cron run as complete |
| `bridge.send` | `{text, chat_id?, service?, append?, type?, file?, caption?, filename?}` | Send message or media to external service (Telegram). `type`: `text`/`photo`/`document`/`voice`/`animation`/`video`/`sticker`. `file`: absolute path for media. `append`: concatenate to previous message. |

## CLI

Most pane commands default to `$CODETERM_PANE_ID` (if set) then fall back to the **current focused pane** when no ID is given.

```bash
codeterm identify                               # System info
codeterm capabilities                           # List API methods

# Workspaces
codeterm workspace list                         # List workspaces
codeterm workspace current                      # Get current workspace ID
codeterm workspace create "Name"                # Create workspace
codeterm workspace focus ID                     # Switch workspace
codeterm workspace rename ID "New Name"         # Rename
codeterm workspace close ID                     # Delete
codeterm workspace move-tab TAB FROM TO             # Move tab between workspaces

# Panes
codeterm pane list                              # List panes
codeterm pane current                           # Get current pane ID
codeterm pane current --last                    # Get last pane ID
codeterm pane focus ID                          # Focus pane
codeterm pane close ID                          # Close pane
codeterm pane split [--id ID] [--pane ID] [--direction right|down]  # Split (default: current)
codeterm pane rename "title" [--pane ID] [--id ID]  # Rename tab (default: current)

# Terminal I/O
codeterm send-text "command" [--pane ID] [--id ID]  # Send text (default: current, --id alias for --pane)
codeterm get-output [--pane ID] [--id ID] [--lines 50]  # Read output (default: current, --id alias for --pane)

# Canvas (visualizations)
codeterm browser open URL                       # Open canvas panel (localhost, data URLs)

# Notifications
codeterm notify "Title" [--body "..."] [--level info|warning|error]
codeterm notifications [--unread]               # List
codeterm notifications clear                    # Clear all

# Themes
codeterm theme list                             # List all themes
codeterm theme apply NAME                       # Apply a theme
codeterm theme save ID FILE [--apply]           # Save custom theme (+ apply)
codeterm theme delete ID                        # Delete custom theme
codeterm theme dir                              # Show themes directory

# Agent Hierarchy
codeterm agent list                            # List all panes with roles
codeterm agent hierarchy                       # Show full hierarchy tree
codeterm agent set-conductor [--pane ID]       # Mark pane as orchestrator (alias: set-orchestrator). Workers/Leads blocked.
codeterm agent link --parent ID [--pane ID]    # Link pane under parent (auto-detects role: Lead under Orchestrator, Worker under Lead)
codeterm agent unlink [--pane ID]              # Clear role. Workers/Leads with a parent CANNOT unlink (backend blocks).
codeterm agent children [--pane ID]            # List children of pane
codeterm agent allow-spawn [--pane ID]         # Toggle can_spawn
codeterm agent report MESSAGE [--pane ID]      # Send report to direct parent (cascade, not to orchestrator). Auto-injected into parent's PTY.
codeterm agent reports [--pane ID] [--clear]   # Read reports from children
codeterm agent status [--pane ID]              # Get detailed status + role
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--workspace WS_ID] [--args A1 A2]
                                               # One-call agent spawn (default agent: claude)
                                               # Role: Orchestrator, Lead, Worker (accepts Conductor/SubWorker as aliases)
                                               # Creates a new tab (does NOT auto-switch active tab)
                                               # --task is baked into launch via -p flag (instant start, no delay)
                                               # --workspace for cross-workspace spawn
                                               # Returns immediately (non-blocking); agent launch happens in background
                                               # Uses socket API (not REST) for reliability
                                               # NOTE: --agent-name is only available via REST API, not CLI

# Cron Jobs
codeterm cron list [--workspace ID]            # List all cron jobs
codeterm cron create "name" --schedule "0 9 * * *" --task "Review TODOs" [--cwd /path] [--description "desc"]
                                               # Create cron job (5-field cron: min hour dom mon dow)
codeterm cron edit ID [--name "new name"] [--schedule "..."] [--task "..."]
                                               # Update cron job fields
codeterm cron delete ID                        # Delete cron job
codeterm cron enable ID                        # Enable cron job
codeterm cron disable ID                       # Disable cron job
codeterm cron trigger ID                       # Manually trigger cron job
codeterm cron history ID                       # Show run history
codeterm cron complete --run-id UUID --summary "result"
                                               # Mark cron run as complete (called by spawned agent)

# Configuration
codeterm config get <key>                      # Get config value (dot-notation, e.g. "integrations.telegram_enabled")
codeterm config set <key> <value>              # Set config value (JSON: true, false, 42, "string", ["array"])

# Bridge (Telegram / External Services)
codeterm bridge send --to CHAT_ID --service telegram "message text"
                                               # Send message to Telegram user/chat
codeterm bridge send --to CHAT_ID --service telegram --append "more text"
                                               # Append to previous message (edits in-place)
codeterm bridge send --to CHAT_ID --type photo --file /path/to/img.png --caption "Look"
                                               # Send photo with caption
codeterm bridge send --to CHAT_ID --type document --file /path/to/f.pdf --filename "report.pdf"
                                               # Send document with custom filename
codeterm bridge send --to CHAT_ID --type voice --file /path/to/audio.ogg
                                               # Send voice/video/animation/sticker

# Screenshot
codeterm screenshot                            # Capture window, print JSON (data URL + dimensions)
codeterm screenshot --output screenshot.jpg    # Capture window, save to file
```

Add `--json` flag for machine-readable output.

### Internal Tauri Commands

These are Tauri IPC commands (invoked from frontend via `invoke()`), not REST endpoints.

| Command | Params | Description |
|---------|--------|-------------|
| `mux_update_pane_cwd` | `pane_id, cwd` | Updates pane's current working directory (called by OSC 7 handler in frontend) |
| `mux_set_pane_agent_type` | `pane_id, agent_type` | Marks pane as running an agent (e.g. `"claude"`) |
| `detect_claude_session_id` | `cwd, exclude?, max_age_secs?` | Finds Claude Code session ID by scanning `~/.claude/projects/` directory structure |
| `mux_set_claude_session_id` | `pane_id, session_id` | Links pane to a Claude Code session for `--resume` support |
| `get_default_cwd` | — | Returns the application's current working directory |
| `mux_batch` | `commands: [{method, params, id?}]` | Execute batch commands with $ref resolution (same semantics as POST /api/batch). Delegates to the unified ServiceContext. |
| `get_autostart_enabled` | — | Returns whether OS autostart (launch at login) is enabled |
| `set_autostart_enabled` | `enabled: bool` | Enable/disable OS autostart. Windows: HKCU registry. macOS: LaunchAgent plist. Linux: .desktop file. |

## Environment Variables

Set automatically in every PTY spawned by CodeTerm:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Running inside CodeTerm |
| `CODETERM_VERSION` | `0.4.0` | App version |
| `CODETERM_SOCKET_PATH` | `\\.\pipe\codeterm-1234` | Socket path |
| `CODETERM_PANE_ID` | `a3f1b2c7` | Current pane ID (8-char hex) |
| `CODETERM_WORKSPACE_ID` | `b4e2c8d1` | Current workspace ID (8-char hex) |
| `CODETERM_ROLE` | `Orchestrator` | Agent role: `Orchestrator`, `Lead`, or `Worker` (empty if no role). Set at PTY spawn. |
| `CODETERM_PARENT_PANE_ID` | `b4e2c8d1` | Parent pane ID in hierarchy (empty if Orchestrator or no role). Set at PTY spawn. |
| `CODETERM_API_PORT` | `7685` | API port for the running instance. Set by CodeTerm so CLI and hooks use the correct port (dev: 7686, prod: 7685). |
| `TERM` | `xterm-256color` | Terminal type |
| `COLORTERM` | `truecolor` | Color support |
| `TERM_PROGRAM` | `CodeTerm` | Terminal name |

## Agent Integration

CodeTerm automatically detects Claude Code and other agents:
- **Permission requests** → in-app toast + Windows notification + webhook
- **Task completion** → in-app toast + Windows notification + webhook
- **Agent icon** on tab (orange ✻ for Claude Code)
- **Pinned tab names** — manual rename blocks OSC auto-rename

Skills for Claude Code are in `skills/` directory. Install via Settings → Agents & Skills.

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, splits, settings, and theme switching.

## System Tray

CodeTerm hides to the system tray on window close instead of quitting. To fully quit, use the tray menu. This ensures cron jobs and background agents continue running when the window is closed.

**macOS:** The tray icon uses a template image (white on transparent) that auto-adapts to light and dark menu bar modes.

## Dev/Prod Isolation

Dev builds (`tauri.dev.conf.json`) use identifier `com.codeterm.dev` and are fully isolated from production:
- **API port:** 7686 (prod: 7685)
- **Data directory:** `~/.codeterm-dev/` (prod: `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (prod: `codeterm`)
- **Single-instance:** Disabled in dev builds, so dev and prod can run side by side.

## Agent Prompts

Agent prompt templates are stored as Markdown files in `src-tauri/prompts/` and loaded at compile time via `include_str!()`. Role-specific suffixes are appended to tasks at spawn time:
- **Worker** (`worker_suffix.md`): Instructions to report back via `codeterm agent report`.
- **Lead** (`lead_suffix.md`): Spawn instructions + "Do NOT sleep or poll for reports" rule.
- **Orchestrator** (`orchestrator_suffix.md`): "Do NOT sleep or poll for reports" rule.
- **Cron tasks** (`cron_complete_suffix.md`): Completion instruction with `cron complete --run-id` command.
- **General Agent** (`general_agent.md`): System prompt for the always-on General Agent.
- **CodeTerm Rule** (`codeterm_rule.md`): Base rules injected into all agent prompts.
- **Extra Instructions** (`extra_instructions.md`): Additional context for agent tasks.
- **Cron Report Hint** (`cron_report_hint.md`): Hint for cron agents to produce structured reports.

<!-- revision: 21 -->
