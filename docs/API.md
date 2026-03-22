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

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/panes/{id}/role` | `{role, parent_pane_id?, can_spawn?}` | Set pane role (`Conductor`, `Worker`, `SubWorker`) |
| DELETE | `/api/panes/{id}/role` | — | Clear role (orphans children) |
| GET | `/api/panes/{id}/status` | — | Pane status + role + children info |
| GET | `/api/panes/{id}/children` | — | List child panes |
| POST | `/api/panes/{id}/report` | `{message}` | Worker sends report to parent conductor |
| GET | `/api/panes/{id}/reports` | `?clear=true` | Conductor reads reports (optionally clearing them) |
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
| `id` | `string` | Unique pane UUID |
| `pty_id` | `string` | Associated PTY process ID |
| `pane_type` | `string` | `"Terminal"` or `"Browser"` (canvas for visualizations) |
| `title` | `string` | Display title (user-pinned or auto-detected) |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `url` | `string?` | URL for canvas panes (localhost, data URLs) |
| `agent_type` | `string?` | Agent running in pane, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |
| `role` | `string?` | `"Conductor"`, `"Worker"`, `"SubWorker"`, or `null` |
| `parent_pane_id` | `string?` | Parent pane ID in hierarchy, or `null` |
| `can_spawn` | `bool` | Whether this pane can spawn child workers |
| `depth` | `number` | Hierarchy depth: 0 (no role), 1 (Conductor), 2 (Worker), 3 (SubWorker) |

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

Custom themes are stored as `.toml` files in `%APPDATA%/codeterm/themes/` (Windows) or `~/.config/codeterm/themes/` (macOS/Linux).

### Agent Spawn (POST /api/agents/spawn)

Creates a tab, assigns a role, launches an agent (claude/aider/any), and optionally sends a task. All in one request.

**Request body:**
```json
{
  "agent": "claude",           // agent command (default: "claude")
  "agent_name": "architect",   // maps to --agent flag (.claude/agents/ file)
  "title": "Worker-1",         // tab title (default: agent_name or agent command)
  "cwd": "C:/project",         // working directory
  "role": "Worker",            // Conductor, Worker, SubWorker
  "parent_pane_id": "uuid",   // parent for Worker/SubWorker
  "can_spawn": true,           // allow spawning children
  "task": "analyze the code",  // baked into launch via -p flag (instant start)
  "args": ["--verbose"],       // extra agent args
  "workspace_id": "uuid"      // optional, defaults to active workspace
}
```

All fields are optional. `agent` defaults to `"claude"`. For Claude, `--dangerously-skip-permissions` is added automatically. When `task` is provided, it is passed via the `-p` flag so the agent starts working immediately (no delay). When `agent_name` is provided, it maps to `--agent <name>` which loads the agent definition from `.claude/agents/<name>.md`.

**Response:**
```json
{"pane_id": "uuid", "tab_id": "uuid", "pty_id": "uuid", "agent": "claude", "title": "Worker-1", "role": "Worker"}
```

### Batch Commands (POST /api/batch)

Execute multiple operations in a single HTTP request. Commands run sequentially. Each command can have an `id` that other commands reference via `$id` (resolves to `pane_id` from that result).

**Request body:**
```json
{
  "commands": [
    {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Conductor", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
    {"method": "spawn", "id": "w1", "params": {"title": "Worker-1", "role": "Worker", "parent_pane_id": "$boss", "agent": "claude", "cwd": "C:/project", "task": "analyze code"}},
    {"method": "spawn", "params": {"title": "Worker-2", "role": "Worker", "parent_pane_id": "$boss", "agent": "claude"}},
    {"method": "spawn", "params": {"title": "Sub-W1", "role": "SubWorker", "parent_pane_id": "$w1", "agent": "claude"}},
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
| `pane.send_text` | `{pane_id, text}` | Send text to PTY |
| `pane.get_output` | `{pane_id, max_bytes?}` | Read terminal output |
| `pane.split` | `{pane_id, direction?}` | Split pane |
| `pane.rename` | `{pane_id, title}` | Rename pane (pinned) |
| `browser.open` | `{url}` | Open canvas panel (localhost, data URLs) |
| `notification.create` | `{title, body?, level?, pane_id?}` | Create notification |
| `notification.list` | `{unread_only?}` | List notifications |
| `notification.clear` | `{id?}` | Clear notifications |

## CLI

Most pane commands default to the **current focused pane** when no ID is given.

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
codeterm workspace move-tab TAB --from WS --to WS  # Move tab

# Panes
codeterm pane list                              # List panes
codeterm pane current                           # Get current pane ID
codeterm pane current --last                    # Get last pane ID
codeterm pane focus ID                          # Focus pane
codeterm pane close ID                          # Close pane
codeterm pane split [--id ID] [--direction right|down]  # Split (default: current)
codeterm pane rename "title" [--id ID]          # Rename tab (default: current)

# Terminal I/O
codeterm send-text "command" [--pane ID]        # Send text (default: current)
codeterm get-output [--pane ID] [--lines 50]    # Read output (default: current)

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
codeterm agent set-conductor [--pane ID]       # Mark pane as conductor
codeterm agent link --parent ID [--pane ID]    # Link pane as worker under parent
codeterm agent unlink [--pane ID]              # Clear role
codeterm agent children [--pane ID]            # List children of pane
codeterm agent allow-spawn [--pane ID]         # Toggle can_spawn
codeterm agent report MESSAGE [--pane ID]      # Send report to parent conductor
codeterm agent reports [--pane ID]             # Read reports (as conductor)
codeterm agent status [--pane ID]              # Get detailed status + role
codeterm agent spawn [AGENT] [--title T] [--cwd DIR] [--role R] [--parent ID] [--can-spawn] [--task TEXT] [--agent-name NAME] [--args A1 A2]
                                               # One-call agent spawn (default agent: claude)
                                               # --agent-name maps to .claude/agents/ file (e.g. "architect")
                                               # --task is baked into launch via -p flag (instant start, no delay)

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

## Environment Variables

Set automatically in every PTY spawned by CodeTerm:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Running inside CodeTerm |
| `CODETERM_VERSION` | `0.1.0` | App version |
| `CODETERM_SOCKET_PATH` | `\\.\pipe\codeterm-1234` | Socket path |
| `CODETERM_PANE_ID` | `uuid` | Current pane ID |
| `CODETERM_WORKSPACE_ID` | `uuid` | Current workspace ID |
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

<!-- revision: 9 -->
