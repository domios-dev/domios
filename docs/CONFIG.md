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

[notifications]
desktop = false     # opt-in; the per-level grid below still decides which levels toast
sound = false       # opt-in chime on an arrival that says the agent stopped
toast_duration = 5  # seconds
show_bell = true    # false removes the indicator; every record is still kept

# Where each level may go BEYOND the panel. The panel is the complete log and
# keeps every record whatever is written here — there is no column for it, and
# no way to route a level away from it. Rows are generated from the same closed
# level set the push channels use, and are reconciled on read: a level added to
# that set appears here on its own, and a row naming no known level is dropped.
# Any combination of columns is legal, including none — that level is kept in
# the panel and interrupts nowhere. A fresh install ships what is written below,
# which interrupts nowhere at every level. An agent notification that names no
# level is kept too, and never reaches the system or a push channel.
# `bell` is RETAINED AND INERT. Nothing routes on it and settings does not offer
# it; it stays a named field because the channel keys are flattened into the
# row, so dropping it would make a stored `bell = true` read back as a push
# channel called "bell" that no registry will ever resolve. `notifications.
# show_bell` is a different field and still works: it hides the indicator.
# `sound` is presentation, not a destination: it has no column here and cannot
# widen where a level goes. It rides a record's arrival, and since every record
# arrives, what sounds is the level alone — only `action_needed` and `failure`,
# the levels that mean the agent stopped, sound at all.
# Settings offers the system notification as a toggle and the push channels as
# ONE picker per level, so a registered channel widens that picker instead of
# adding a row. Only a channel the daemon reports as able to carry a message is
# offered. Unpicking a channel REMOVES its key from the row rather than writing
# `false`; both read as closed, which is what an unnamed channel has always been.
[[notifications.agent_levels]]
level = "verbose"
bell = true
desktop = false
[[notifications.agent_levels]]
level = "action_needed"
bell = true
desktop = false
# telegram = true      # one flag per registered push channel, keyed by service id
[[notifications.agent_levels]]
level = "failure"
bell = true
desktop = false

`desktop` is the one local surface a row routes to. Every other key on a row is
a push channel's service id, and a channel key that is absent reads as closed —
a column nobody named is a column nobody chose, so a channel the registry gains
arrives off and stays off until someone opts into it. The same answer serves the
shipped default, which is why a config written before the grid existed now reads
as interrupting nowhere rather than as whatever the surrounding switches allowed
then.

`bell` on a row is **retained and inert**. It is written above because the field
still exists in the stored shape, and it is deliberately not removed: the channel
keys are flattened onto the row, so a row that stopped naming `bell` would read
a stored `bell = true` back as a push channel called `bell` that no registry will
ever resolve. Nothing routes on it, settings does not offer it, and changing it
changes nothing. It used to gate the panel's only live event, which meant a
`bell = false` level was recorded while the panel silently lagged until the next
mount — not "this level does not reach the bell", which is why the column is gone.

`show_bell` is the bell answer config still makes, and it is presentation:
`show_bell = false` removes the indicator from the titlebar while every
notification is still recorded, so turning it back on reveals the complete log.

Everything under `notifications` is set from Settings, not from `codeterm
config set`. Where a notification may go is your standing answer, so an agent
that could switch a desktop toast or a push channel on by itself would grant
itself a capability you never saw. A `codeterm config set notifications.*` — and
any config write arriving from inside a tab that carries a `notifications`
section — is refused with `403`. An agent may OFFER instead: `codeterm notify
send "…" --choice "Turn it on" --choice "Not now" --offer desktop --offer-accept
"Turn it on"` sends a question, and the surface changes only when you tap the
accepting label. An untapped or declined offer changes nothing. The refusal is
not agent-specific — the daemon cannot tell you from an agent inside a tab, so
the same command typed by hand in a CodeTerm tab is refused too.

# Per-channel push AUDIENCE, keyed by the channel's bridge service id — the same
# id `codeterm bridge send --service <id>` takes. Whether a level reaches a
# channel at all is that channel's column of the level grid above; this answers
# only who receives what the grid routed here. A channel with no entry has no
# audience, and an empty audience is nobody: it never falls back to that
# integration's allowed-users list, which admits inbound senders instead.
# Settings offers those admitted ids as the choices, and stores only what you pick.
[notifications.channels.telegram]
recipients = []        # explicit audience (chat ids) — NOT telegram_allowed_users
per_scope_limit = 12   # lifetime push ceiling per originating tab (a count, not a rate)

[orchestration]
plan_reminders = true
nudge_interval_secs = 1800  # 30 minutes

[orchestration.reflect]
every_gates = 15  # emit a one-shot self-review block every N gate/bounce events (0 disables)

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
persist_master_password = false              # Save master password in OS keychain on unlock (auto-unlock after restart). Off by default.
```

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

## Per-repo lane bootstrap

A repository can declare lane bootstrap in `<repo>/.codeterm/config.toml` (repo-level, separate from `~/.codeterm/config.toml`). Exactly two keys:

```toml
[lane]
shared_directories = ["node_modules"]  # gitignored dirs linked from the primary checkout
setup_command = "npm install"          # queued as the lane's first shell command
```

When a Worker lane worktree is created, each declared directory that exists in the primary checkout **and** is gitignored in the lane is linked into the lane (symlink; directory junction fallback on Windows). A name that is not gitignored is refused — the link must stay invisible to `git status` so `git add -A` cannot stage it. Link problems are recorded as warnings in the lane-dispatch record and never fail the dispatch. The setup command runs before the agent launch as an ordinary shell command; its failure is visible in the lane tab. Bounded parse: at most 100 entries; absolute paths, drive-letter paths, `..`/`.` segments, and `.git` are rejected. Deleting the keys restores current behavior.

## Related documents

Integration settings: `codeterm docs INTEGRATIONS`. Runtime, UI, and memory settings: `codeterm docs RUNTIME-CONFIG`.

<!-- revision: 50 -->
