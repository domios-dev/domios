# CodeTerm API Reference

CodeTerm exposes three APIs for agent integration:
1. **REST API** — HTTP on port 7685 (configurable, binds to 0.0.0.0 by default)
2. **Socket API** — JSON-RPC v2 over named pipe (Windows) / Unix socket
3. **CLI** — `codeterm.exe` command-line tool

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

### Pane Fields

Fields returned by `GET /api/panes` for each pane:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique pane UUID |
| `pty_id` | `string` | Associated PTY process ID |
| `pane_type` | `string` | `"terminal"` or `"browser"` |
| `title` | `string` | Display title (user-pinned or auto-detected) |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `url` | `string?` | URL for browser panes |
| `agent_type` | `string?` | Agent running in pane, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |

#### Browser
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/browser/open` | `{url}` | Open URL in browser panel |
| GET | `/api/serve-file` | `?path=...` | Serve local file (for iframe) |

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

Webhook events: `agent.permission_request`, `agent.task_complete`

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

### Examples

```bash
# Health check
curl http://host:7685/api/health

# Create workspace + tab
WS=$(curl -s -X POST http://host:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"my-project"}' | jq -r '.id')

# Get pane and run command
PANE=$(curl -s http://host:7685/api/panes | jq -r '.[-1].id')
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
# {"local_ip":"192.168.1.195","tailscale_ip":"100.84.159.37","api_url":"http://192.168.1.195:7685"}
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
| `browser.open` | `{url}` | Open browser panel |
| `notification.create` | `{title, body?, level?, pane_id?}` | Create notification |
| `notification.list` | `{unread_only?}` | List notifications |
| `notification.clear` | `{id?}` | Clear notifications |

## CLI

```bash
codeterm identify                               # System info
codeterm capabilities                           # List API methods

# Workspaces
codeterm workspace list                         # List workspaces
codeterm workspace create "Name"                # Create workspace
codeterm workspace focus ID                     # Switch workspace
codeterm workspace rename ID "New Name"         # Rename
codeterm workspace close ID                     # Delete
codeterm workspace move-tab TAB --from WS --to WS  # Move tab

# Panes
codeterm pane list                              # List panes
codeterm pane focus ID                          # Focus pane
codeterm pane close ID                          # Close pane
codeterm pane split ID [--direction right|down] # Split
codeterm pane rename ID "title"                 # Rename tab

# Terminal I/O
codeterm send-text PANE_ID "command"            # Send text
codeterm get-output PANE_ID [--lines 50]        # Read output

# Browser
codeterm browser open URL                       # Open browser panel

# Notifications
codeterm notify "Title" [--body "..."] [--level info|warning|error]
codeterm notifications [--unread]               # List
codeterm notifications clear                    # Clear all
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

<!-- revision: 3 -->
