# CodeTerm Configuration

Config file: `%APPDATA%/codeterm/config.toml` (Windows) or `~/.config/codeterm/config.toml` (Linux/macOS).

Auto-created on first run with defaults.

## Full Reference

```toml
# CodeTerm Configuration

[theme]
name = "github-dark"
# Available: github-dark, github-light, dracula, catppuccin-mocha, one-dark, nord, solarized-light, pip-boy
# Override individual colors:
# background = "#0d1117"
# foreground = "#c9d1d9"
# cursor = "#58a6ff"

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"  # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[diagnostics]
terminal_transport = false

# [notifications] — levels, surfaces, sound, push channels.
# Its own reference, it outgrew this file: codeterm docs NOTIFICATIONS


[orchestration]
plan_reminders = true
nudge_interval_secs = 2700  # 45 minutes

[orchestration.reflect]
every_gates = 15  # emit a one-shot self-review block every N gate/bounce events (0 disables)

[orchestration.map]
enabled = false  # opt in to the workspace essence map and its boundary Cartographer

[keybindings]
new_tab = "Ctrl+T"
reopen_tab = "Ctrl+Shift+T"
close_tab = "Ctrl+W"
next_tab = "Ctrl+Tab"     # cycle forward through plan entry + session tabs in sidebar order
prev_tab = "Ctrl+Shift+Tab"  # cycle backward
new_workspace = "Ctrl+Shift+N"
copy = "Ctrl+Shift+C"
paste = "Ctrl+Shift+V"
search = "Ctrl+Shift+F"
zoom_in = "Ctrl+="
zoom_out = "Ctrl+-"
zoom_reset = "Ctrl+0"
open_settings = "Ctrl+,"

[socket]
# Auto-generated, override if needed:
# path = "\\\\.\\pipe\\codeterm-custom"

[agent]
# Agent config controls AI agent integration (e.g. Claude Code).
# poll_interval_ms: how often the agent polls for new commands.
# idle_timeout_s: seconds of idle before treating agent as idle (used for output streaming).
# resume_sessions: auto-resume agent sessions on restart. When the JSONL referenced
#                  by the saved session id is missing on disk (`AgentProvider::session_exists`
#                  returns false), CodeTerm clears the stale id and launches fresh
#                  instead of looping on "No conversation found".
# full_access: one global permission choice for every agent provider.
# Omit it until onboarding or the first agent launch asks the user.
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true
full_access = true

[api]
enabled = true
host = "127.0.0.1"
port = 7685
# Auto-generated on first load; every local control request uses Bearer auth.
# token = "your-secret-token"
cors = false
# cors_allow_origins = ["https://trusted.example"]

[general_agent]
enabled = true
agent_provider = "claude"       # Agent command to use
auto_restart = true             # Auto-restart on crash
max_restart_attempts = 3
restart_delay_s = 60

[integrations]
telegram_enabled = false
telegram_allowed_users = []     # Telegram user IDs (strings), empty = allow all
discord_enabled = false
discord_allowed_users = []
webhook_enabled = false
webhook_port = 7687

[integrations.secrets]
backend = "local"                # "local" (default), "bitwarden", or "mock" (dev only)
default_organization = ""        # Default org id for new items (Bitwarden); empty = personal vault
default_collection = ""          # Default collection id for new items (Bitwarden)

[integrations.secrets.bitwarden]
server_url = "https://vault.bitwarden.com"   # Public vault, or self-hosted Vaultwarden URL

[kpi]
enabled = true                   # Local orchestration-economics ledger; false opts out
```

## Terminal transport diagnostics

`diagnostics.terminal_transport` is off by default. When enabled, the daemon
writes bounded `[terminal-stream]` session summaries for attach cursors, frame
and output-byte counts, offsets, truncation and boot changes. It also writes
`[pty-write-route]` lines with the tab, PTY id, request byte count and whether
that PTY is current for the tab. These diagnostics never include terminal
output or input text, tokens, or credentials.

Toggle it live with `codeterm config set diagnostics.terminal_transport true|false`.
The next output-stream session and PTY write use the new setting; an open stream
keeps the setting it started with. For mesh traces, enable it on both the local
relaying daemon and the peer. No restart is required.

`kpi.enabled` owns the ledger behind Mission Control's Tokens and Turns cards. It
records plan, task and tab ids, event kinds, byte counts, token counts and
timestamps in `~/.codeterm/kpi_ledger.db` — never message text, and nothing leaves
the machine. Opt out with `codeterm config set kpi.enabled false`; opt back in with
`codeterm config set kpi.enabled true`. A config saved before KPI defaulted on still
carries the old `enabled = false`; the first load after upgrading switches it on once
and records `kpi.default_applied = true`, so an opt-out saved afterwards is kept.
Either way the change takes effect on the
next CodeTerm start, and while it is off the Board shows an explicit not-recording
state naming this key rather than a zero.

## Plugin settings

Plugin-defined settings live under `config.plugins.<id>` and are preserved as a
plugin-specific settings bag. The Extensions list and titlebar Plugins button
open installed plugin pages; view plugins render their embedded page there.

The API listens only on loopback by default. A non-loopback `host` requires a
non-empty Bearer token. The existing Mesh switch is the only network-access
control: Mesh off rejects every remote request while localhost remains available;
Mesh on permits authenticated remote access. The installed app's WebView origins
are always allowed to reach its local daemon. Other cross-origin clients remain
closed unless `cors = true` and every allowed HTTP(S) origin is named in
`cors_allow_origins`; `*` is never accepted. Authenticated
`GET /api/config` redacts the token and reports both the configured address and
the listener policy the process actually owns. `localhost`, `::1`, and `[::1]`
are canonicalized to bindable loopback socket addresses; other hostnames are
rejected in favor of an explicit numeric address.

Named development instances may explicitly set `api.mesh_tailnet_host` to a
locally assigned Tailscale IPv4 address. This adds an authenticated listener
while retaining the loopback primary; the default is absent. Production and
unnamed instances refuse this option. A restart is required. Both sockets must
bind and pass health checks before remote readiness is published; startup never
falls back to a LAN or wildcard bind. If the extra listener exits after readiness,
loopback remains serving, remote readiness is withdrawn, and broadcasting stops.
`api_listener.ready_addresses` is the live health-proven set, not the configured
request; `requested_mesh_tailnet_address` records the startup request only.

Agent access is one global choice. `agent.full_access = true` applies the
provider's permission-bypass capability whenever any agent starts or restarts,
including General Agent; `false` keeps provider permission prompts everywhere.
The first-run picker asks once with Full access selected by default;
there are no per-provider, per-resume, audience, or Remember-choice policies.
On first load, an enabled remembered bypass from the former global or
per-provider settings is converted to `agent.full_access = true` before any
agent tab starts; the removed keys are then deleted from the saved config.

## Config API

Read and write config values at runtime via REST or CLI.

**REST:**
```bash
# Read full config
curl http://host:7685/api/config

# Read a specific key (dot-notation)
curl "http://host:7685/api/config?key=integrations.telegram_enabled"

# Set a value
curl -X PATCH http://host:7685/api/config \
  -H "Content-Type: application/json" \
  -d '{"key": "integrations.telegram_enabled", "value": true}'
```

**CLI:**
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent              # read an entire section
codeterm config get                            # no key → dump the whole redacted config
```

`codeterm config get` with no key omits the `?key=` query and prints every key/value (secrets redacted). Passing a key scopes the read to that dot-path.

Values are JSON-compatible: `true`, `false`, `42`, `"string"`, `["array"]`. Changes are saved to disk immediately and trigger a `config-changed` event.

## General Agent

The General Agent is an always-on Claude Code instance that lives in the "General" workspace. It handles Telegram messages, cron tasks, and acts as the default orchestrator.

```toml
[general_agent]
enabled = true                  # Enable the General Agent
agent_provider = "claude"       # Agent command (default: claude)
auto_restart = true             # Auto-restart if agent crashes
max_restart_attempts = 3        # Max restart attempts before giving up
restart_delay_s = 60            # Seconds between restart attempts
```

When enabled, CodeTerm starts with a single "General" workspace instead of the default "Main" workspace. The General Agent tab is marked as protected (cannot be closed via UI).

Settings UI: Settings > Agents > General Agent section (enable, auto-restart toggles).

## Worker lane provisioning

Worker lanes are source-only. Provisioning creates an isolated git worktree and
launches the agent there; it does not install dependencies or link any directory
from the canonical checkout. In particular, `node_modules` is never shared by a
symlink or Windows junction, and no package-manager command is placed before the
agent command. Source-only workers do not build or test in their lane.

## Related documents

Notification settings: `codeterm docs NOTIFICATIONS`. Integration settings: `codeterm docs INTEGRATIONS`. Runtime, UI, and memory settings: `codeterm docs RUNTIME-CONFIG`.

<!-- revision: 63 -->
