# CodeTerm Skills for Claude Code

CodeTerm ships skills that teach Claude Code how to control the terminal.

## Installation

Copy skills to your Claude Code skills directory:

```bash
cp -r skills/codeterm ~/.claude/skills/
```

Or symlink:
```bash
ln -s /path/to/codeterm/skills/codeterm ~/.claude/skills/codeterm
```

## Available Skills

### `codeterm` — Core terminal control
- Create tabs, manage workspaces
- Send commands to other panes
- List sessions and panes

## How it works

Skills are markdown files that Claude Code loads automatically when relevant. When Claude detects it's running in CodeTerm (via `$CODETERM=1` env var and `$CODETERM_PANE_ID` for self-identification), it can use the `codeterm` CLI to:

1. **Send commands**: `codeterm send "npm test"` (sends immediately to current pane, newline appended automatically). For quote-heavy or multiline text, use `codeterm send --stdin --pane ID` and pipe/heredoc the payload. For terminal keys, use `codeterm send --actions "esc,delay:200ms,up,enter" --pane ID` (`enter`, arrows, `tab`, `backspace`, `ctrl-c`, `key*3`, and `delay:250ms` are supported). To schedule a future pane message without blocking yourself, use `codeterm send --delay 10m --pane ID "text"`, `--delay 4h`, `--delay 1d`, or `--at 2026-06-15T09:00`; manage queued items with `codeterm delayed list|edit|send-now|delete`.
2. **Send notifications**: `codeterm notify "Done" --level info`
3. **Create workspaces**: `codeterm workspace create "Feature X"`
4. **Read output**: `codeterm get-output` (reads from current pane)
5. **Rename tab**: `codeterm pane rename "my task"` or `codeterm pane rename "my task" --pane ID`
6. **Create theme**: `codeterm theme save my-theme /tmp/theme.toml --apply`
7. **Agent hierarchy**: `codeterm agent set-orchestrator` (mark self as orchestrator)
8. **Compact context**: `codeterm agent compact [--pane ID]` (sends /compact to Claude)
9. **Choose model**: use `codeterm agent models <provider>` to list valid ids and `codeterm agent spawn <provider> --model MODEL --task "..."` to select one. Do not treat `codeterm agent model` as a universal live switch.
10. **Send report**: `codeterm agent report "task done"` (report to direct parent, auto-injected into parent's PTY)
11. **List panes / hierarchy**: `codeterm pane list` (workspace tree, sub-agents nested; `--agents` filters to agent panes)
12. **Spawn agent**: `codeterm agent spawn claude --title "Worker" --role Worker --parent ID --task "do work"` (creates new tab, non-blocking, does NOT switch active tab; task delivery is provider-owned and is not an inter-pane `-=-codeterm:pane...-=-` message)
13. **Spawn with agent definition**: via REST API `POST /api/agents/spawn` with `agent_name` field (loads `.claude/agents/<name>.md`; not available in CLI)
14. **Cross-workspace spawn**: `codeterm agent spawn claude --workspace WS_ID --title "Remote" --role Worker --parent ID --task "work"`
15. **Create cron job**: `codeterm cron create "name" --schedule "0 9 * * *" --task "Review TODOs"` (uses `$CODETERM_WORKSPACE_ID` by default; add `--once` for oneshot: `--schedule "in 30m" --once`)
16. **Edit cron job**: `codeterm cron edit ID --schedule "*/15 * * * *"` (prefer over delete+recreate to preserve history)
17. **Complete cron run**: `codeterm cron complete --run-id UUID --summary "result"`
18. **List cron jobs**: `codeterm cron list`, **trigger manually**: `codeterm cron trigger ID`
19. **Read config**: `codeterm config get integrations.telegram_enabled` (dot-notation)
20. **Write config**: `codeterm config set integrations.telegram_enabled true` (JSON values)
21. **Send to Telegram**: `codeterm bridge send --to USER_ID --service telegram "text"` (auto-edits within 60s, `--append` to concatenate)
22. **Keep system awake**: `codeterm keepawake start --duration-secs 3600 [--display]` (cross-platform: caffeinate on macOS, SetThreadExecutionState on Windows). Stop: `codeterm keepawake stop`. Status: `codeterm keepawake status`. Use this instead of calling `caffeinate` / `powercfg` / `systemd-inhibit` directly.
23. **Search past agent sessions**: `codeterm sessions search "QUERY" [--provider claude|codex|all] [--project /path] [--limit N]` — content search across every registered provider's on-disk sessions. Each hit carries its `provider` and `session_id`.
24. **Inspect a past session**: `codeterm sessions get SESSION_ID` — provider, recorded cwd, file path, timestamps. Use before `resume` to verify the JSONL is still on disk.
25. **Resume a past session atomically**: `codeterm sessions resume SESSION_ID [--title T] [--workspace-id ID] [--workspace-name N]` — locates the session, opens the right workspace at the recorded cwd, spawns the agent pane, and hands the resume id to the provider with the correct CLI shape. Use this instead of the brittle "create workspace at guessed cwd → `agent spawn --args=--resume=UUID`" dance.
26. **Manage local services (daemon registry)**: `codeterm service list [--kind cron|daemon]`, `codeterm service add NAME --health-mode http|tcp|process|script [...] [--push]` (registers a daemon for the Mesh tab; `--push` returns a per-service Bearer token printed ONCE for `POST /api/panes/<id>/send`), `codeterm service show ID`, `codeterm service edit ID --enabled true|false`, `codeterm service delete ID`, `codeterm service token ID --rotate` (old token invalidated immediately, also auto-enables push).
27. **Memory pin / unpin**: `codeterm mem pin <id>` and `codeterm mem unpin <id>` toggle `is_pinned_wakeup` so the item is always loaded into the L1 wake-up payload (subject to budget). `codeterm mem pinned [--limit N]` lists every pinned item.
28. **Atomic pinned save**: `codeterm mem save --kind fact --title "T" --pin --workspace $CODETERM_WORKSPACE_ID "<body>"` creates a pinned, workspace-scoped item in a single round-trip — no follow-up PATCH.
29. **Workspace-aware wake-up**: `codeterm mem wake-up --workspace $CODETERM_WORKSPACE_ID` boosts items scoped to that workspace by `+0.5` in the L1 score and admits them alongside global rows. Add `--explain` to append a per-item score breakdown for debugging selection.
30. **Calibration audit / manual run**: `codeterm mem calibration log [--since ISO] [--limit N] [--count]` shows the dreamer's importance / archive sweep history. `codeterm mem calibration sweep [--force]` runs it manually — server-gated to Sunday unless `--force`, hard-capped at 30 mutations per session.
31. **Audit-trail actor**: `codeterm mem update <id> --changed-by <actor> ...` sets the `X-CodeTerm-Actor` header so `mem_item_history.changed_by` carries the actor label (e.g. `dreamer-calibration:promote`). Default is `"user"` when the flag is omitted.

## CLI Flags

All CLI commands support the `--json` flag for machine-readable output:
```bash
codeterm pane list --json
codeterm workspace list --json
```

## Integrations

### Telegram Bridge
CodeTerm routes messages between Telegram and the General Agent:
- **Inbound:** Messages arrive as `[Telegram: user_id] text` in the agent's PTY. Rapid messages within 300ms are debounced. Typing indicator is sent automatically.
- **Outbound:** Agent sends replies via CLI: `codeterm bridge send --to USER_ID --service telegram "text"`. Within 60s, messages auto-edit the previous one. Use `--append` to concatenate text.

Setup via CLI:
```bash
# Save token to secrets dir
mkdir -p ~/.codeterm/secrets
echo -n "BOT_TOKEN" > ~/.codeterm/secrets/telegram.token

# Enable
codeterm config set integrations.telegram_enabled true
codeterm config set integrations.telegram_allowed_users '["USER_ID"]'

# Restart CodeTerm (hot-reload not yet supported)
```

Sending messages from the agent:
```bash
# Send a new message
codeterm bridge send --to 123456789 --service telegram "Hello from CodeTerm"

# Append to the previous message (edits in-place)
codeterm bridge send --to 123456789 --service telegram --append "...continued"
```

Connected panes show service icons in the sidebar (`connected_services` field).

#### Hot-relocating the poller (`bridge migrate`)

Telegram (and any future single-poller bridge) holds an exclusive long-poll lock against the upstream API — only one poller per bot token at a time. `bridge migrate` moves that poller to a paired mesh peer without restarting either daemon. **Run on the source machine**; `--mesh PEER bridge migrate` exits `6`.

```bash
# Move the active Telegram bridge from this host to the paired peer "mac-mini"
codeterm bridge migrate --to mac-mini

# Same, but also post a sentinel "now serving from mac-mini" to the maintainer's chat
codeterm bridge migrate --to mac-mini --announce

# Tighten the upstream-lock claim window (default 10000 ms)
codeterm bridge migrate --to mac-mini --claim-timeout-ms 5000

# Skip the target preflight (no token + no vault entry → still try)
codeterm bridge migrate --to mac-mini --force

# Don't auto-rollback if the target claim fails (leave source disabled)
codeterm bridge migrate --to mac-mini --no-rollback
```

| Flag | Default | Description |
|------|---------|-------------|
| `--to` | *(required)* | Target peer `instance_name` or `instance_id` from `codeterm mesh peers`. |
| `--service` | `telegram` | Which bridge to migrate. Only `telegram` today. |
| `--announce` | `false` | Post a one-shot `[CodeTerm] Telegram bridge migrated to <to> from <from>` sentinel after success. |
| `--announce-chat-id` | `186809246` | Telegram chat id for `--announce`. |
| `--no-rollback` | `false` | Skip source-recovery on failure (leaves source disabled). |
| `--force` | `false` | Skip the target preflight that requires `token_present` or `has_secret_in_vault`. |
| `--claim-timeout-ms` | `10000` | How long the target has to claim the upstream lock before the migration is failed. |

**Exit codes:** `0` success / `1` generic failure (rollback OK) / `5` preflight (target unreachable, no token + no vault, missing peer) / `6` usage error (e.g. `--mesh`) / `7` split-brain (rollback also failed — see CLI output for the manual repair command). Each daemon resolves its own vault — secrets never cross the mesh, matching `--mesh PEER mem secret …` (also exit `7`). Full orchestration steps live in `docs/MESH.md`.

### Config Management
Read/write CodeTerm config at runtime:
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent                    # read entire section
```
Dot-notation for nested keys. Values are JSON (true, false, 42, "string", ["array"]).

## Agent Integration

External agents (Rollo, OpenClaw, CI/CD) can connect via REST API:

```bash
# Create workspace for agent
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"Agent"}'

# Create tab
curl -X POST http://localhost:7685/api/workspaces/$WS_ID/tabs

# Send command (enter: true is default, appends \r automatically)
curl -X POST http://localhost:7685/api/panes/$PANE_ID/send \
  -H "Content-Type: application/json" \
  -d '{"text":"npm run build"}'

# Press terminal keys / navigation actions
curl -X POST http://localhost:7685/api/panes/$PANE_ID/send \
  -H "Content-Type: application/json" \
  -d '{"actions":[{"type":"key","key":"esc"},{"type":"delay","ms":200},{"type":"key","key":"enter"}]}'

# Read output
curl http://localhost:7685/api/panes/$PANE_ID/output

# Notify
curl -X POST http://localhost:7685/api/notifications \
  -H "Content-Type: application/json" \
  -d '{"title":"Build complete","level":"info"}'
```

## Agent Hierarchy

CodeTerm has a 2-level role hierarchy: Orchestrator (depth 1) -> Worker (depth 2). Each agent gets its own tab; the hierarchy is a logical chain, not visual splits. IDs are 8-char hex (e.g. `a3f1b2c7`).

**Roles:**
- **Orchestrator** (was Conductor) -- top-level, spawns Workers.
- **Worker** -- executes one task, reports back. Cannot spawn. Cannot call `set-orchestrator`, `unlink`, or `agent spawn`.

Backward-compat alias: `Conductor` -> `Orchestrator`.

**Push-based reports:** Reports go to the spawning Orchestrator, auto-injected into its PTY as `[Report from Name] message\r`, so Claude sees them as user input -- no polling needed.

```bash
# Mark current pane as orchestrator
codeterm agent set-orchestrator --pane $CODETERM_PANE_ID

# View panes as a workspace tree (sub-agents nested); --agents = agent panes only
codeterm pane list

# Get one pane's agent state (type, session, prompt)
codeterm pane status --pane $WORKER

# Clear role: detach a Worker from its Orchestrator (or orphan an Orchestrator's Workers)
codeterm agent unlink --pane $CODETERM_PANE_ID
```

The `agent.report` webhook event fires when any agent sends a report, enabling async orchestration patterns.

### One-call agent spawn

The `task` field is baked into the launch command via the `-p` flag, so the agent starts working immediately with no delay. Spawn returns immediately (non-blocking) -- PTY readiness and agent launch happen in a background thread. Each spawned agent gets its own tab (does NOT auto-switch active tab). Uses socket API for reliability.

```bash
# Spawn a Worker under the Orchestrator
codeterm agent spawn claude --title "Worker-1" --role Worker --parent $CODETERM_PANE_ID --task "run tests"

# Spawn defaults to claude if no agent specified
codeterm agent spawn --title "Helper" --role Worker --parent $CODETERM_PANE_ID

# Cross-workspace spawn
codeterm agent spawn claude --workspace WS_ID --title "Remote" --role Worker --parent $CODETERM_PANE_ID
```

Returns: pane_id, tab_id, pty_id, agent, title, role.

The `--agent-name` option (for loading `.claude/agents/<name>.md`) is only available via the REST API (`POST /api/agents/spawn`), not the CLI.

### Batch API (REST and Tauri IPC)

Spawn an Orchestrator and its Workers in one call. The batch API is available via both `POST /api/batch` (REST) and the `mux_batch` Tauri IPC command, with identical semantics and $ref resolution. Both delegate to the unified ServiceContext.

```bash
curl -X POST http://localhost:7685/api/batch \
  -H "Content-Type: application/json" \
  -d '{
    "commands": [
      {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "can_spawn": true}},
      {"method": "spawn", "params": {"title": "W1", "role": "Worker", "parent_pane_id": "$boss", "task": "analyze code", "agent_name": "reviewer"}},
      {"method": "spawn", "params": {"title": "W2", "role": "Worker", "parent_pane_id": "$boss", "task": "run tests"}}
    ]
  }'
```

Available batch methods: `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`. Use `$id` references to chain command results (e.g. `"parent_pane_id": "$boss"` resolves to the pane_id from the "boss" spawn).

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, splits, settings, and theme switching.

## Session Persistence

CodeTerm tracks and restores terminal state across sessions:

- **CWD tracking**: Working directory is tracked via OSC 7 escape sequences and saved/restored per pane on restart.
- **Claude Code auto-resume**: Claude Code sessions are detected and automatically resumed with `--resume`. Before injecting the resume flag, CodeTerm validates the session file exists on disk via `AgentProvider::session_exists` — missing JSONLs (cleared cache, machine-synced config without sessions) launch fresh instead of looping on `No conversation found with session ID`.
- **Pre-generated session ids**: For providers reporting `SessionIdInjection != None` (Claude today), CodeTerm pre-generates a UUID and passes it via `--session-id` at launch. Each pane gets a unique id even when several panes share the same cwd.
- **SessionStart hook**: Claude's `SessionStart` matcher POSTs back to `/api/panes/{id}/hook/session_start`, so `/new` and `/clear` inside the TUI rotate the persisted `agent_session_id` immediately.
- **Cross-provider session search**: `codeterm sessions search` walks every registered provider's on-disk sessions (Claude JSONLs, Codex `~/.codex/sessions/YYYY/MM/DD/*.jsonl`). `codeterm sessions resume` opens the pane in the correct cwd and hands the resume id to the right provider.
- **Auto-save**: Session state is persisted every 30 seconds.

<!-- revision: 31 -->
