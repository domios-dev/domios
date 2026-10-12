# Domios Configuration

Config file: `~/.codeterm/config.toml` on every platform (dev instances use `~/.codeterm-dev/config.toml` or `~/.codeterm-dev-<name>/config.toml`), auto-created with defaults on first run. `codeterm identify` shows the active data directory.

## Full Reference

```toml
[theme]
name = "github-dark"
# Available: github-dark, github-light, dracula, catppuccin-mocha, one-dark, nord, solarized-light, pip-boy
# Override colors: background / foreground / cursor = "#rrggbb"

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"  # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

# Optional per-Team utility limits; omitted purposes are unlimited.
[utility_tabs.max_per_team]
build = 2
qa = 2
[diagnostics]
terminal_transport = false
protocol_recording = false

# [notifications]: levels, surfaces, sound and push — codeterm docs NOTIFICATIONS

[orchestration]
plan_reminders = true
nudge_interval_secs = 2700  # 45 minutes

[orchestration.reflect]
every_gates = 15  # emit a one-shot self-review block every N gate/bounce events (0 disables)

[orchestration.map]
enabled = false  # opt in to the workspace essence map and its boundary Cartographer

[keybindings]  # Ctrl maps to Cmd on macOS
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
# Optional socket override: path = "\\\\.\\pipe\\codeterm-custom"

[agent]
# poll_interval_ms: how often the agent polls for new commands.
# idle_timeout_s: seconds of idle before treating agent as idle (used for output streaming).
# resume_sessions: resume agent sessions on restart (default true; [agents.<id>] overrides);
#                  a session whose JSONL is missing, or false, launches fresh.
# continue_after_restart: one system continue per mid-turn agent tab after any stop.
# full_access: one global permission choice for every agent provider.
# Omit it until onboarding or the first agent launch asks the user.
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true
continue_after_restart = true  # false disables it (codeterm docs TAB-LIFECYCLE)
full_access = true

[agents.codex]
structured_control_enabled = true # Default; false selects plain PTY

[supporter]
adapter = "typesafe-jev"
deadline_ms = 3000
calls_per_hour = 0 # Unlimited; omit for the same default
[supporter.connections.typesafe-jev]
model = "jev-latest"
[supporter.memory_rerank]
request_mode = "per_candidate"
docs_hybrid = false

[api]
# cors_allow_origins = ["https://trusted.example"]
enabled = true
host = "127.0.0.1"
port = 7685
# Auto-generated on first load; every local control request uses Bearer auth.
# token = "your-secret-token"

[general_agent]
enabled = true
agent_provider = "claude"       # Agent command to use
auto_restart = true             # Auto-restart on crash
max_restart_attempts = 2       # Effective maximum is two per recovery episode
restart_delay_s = 60           # Retained; event recovery does not wait

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

`diagnostics.terminal_transport` is off by default. When enabled, the daemon writes bounded `[terminal-stream]` session summaries for attach cursors, frame and output-byte counts, offsets, truncation and boot changes. It also writes `[pty-write-route]` lines with the tab, PTY id, request byte count and whether that PTY is current for the tab. These diagnostics never include terminal output or input text, tokens, or credentials.

Toggle it live with `codeterm config set diagnostics.terminal_transport true|false`. The next output-stream session and PTY write use the new setting; an open stream keeps the setting it started with. For mesh traces, enable it on both the local relaying daemon and the peer. No restart is required.

## Protocol recording

`diagnostics.protocol_recording` is off by default and only records in a named development instance. When enabled, resolved hook posts are written as redacted NDJSON under `logs/protocol/<tab>/`. A single background writer keeps at most ten 10 MiB files per tab, 512 MiB total, and 14 days of history. A full writer queue drops the record and increments the recorder's dropped counter; hook responses never wait for disk writes. Toggle it live with `codeterm config set diagnostics.protocol_recording true|false`.

`kpi.enabled` owns the ledger behind Mission Control's Tokens and Turns cards. It records plan, task and tab ids, event kinds, byte counts, token counts and timestamps in `~/.codeterm/kpi_ledger.db` — never message text, and nothing leaves the machine. Opt out with `codeterm config set kpi.enabled false`; opt back in with `codeterm config set kpi.enabled true`. A config saved before KPI defaulted on still carries the old `enabled = false`; the first load after upgrading switches it on once and records `kpi.default_applied = true`, so an opt-out saved afterwards is kept. Either way the change takes effect on the next Domios start, and while it is off the Board shows an explicit not-recording state naming this key rather than a zero.

Stored model defaults stay on models their provider still offers. These are `agents.<id>.default_model`, `memory.extraction_model`, `memory.dreaming_model`, `memory.worker_model` and each scheduled job's `model`. At startup and on every model catalogue change, the daemon checks each default against its provider's catalogue, using the same matching as spawn. For a provider with an authoritative catalogue, a default it no longer offers is dead. A free-form catalogue (one that accepts unlisted ids) only counts a default as dead once it stops listing a model it listed earlier. A provider that needs live discovery is checked only against a live snapshot. A dead default moves to the newest model in the same family; failing that, to the provider's recommended model; failing that, to the first model it offers. One notification lists every change. `model_defaults.replaced.<setting path>` records `from`, `to` and the reason. Settings → Agents → Model defaults shows "Updated automatically from …" while the replacement is still the configured value. Supporter decision-adapter models are not checked: their adapters publish no authoritative catalogue and no change event.

## Plugin settings

Plugin-defined settings live under `config.plugins.<id>` and are preserved as a plugin-specific settings bag. The Extensions list and titlebar Plugins button open installed plugin pages; view plugins render their embedded page there.

Agent-facing plugin settings use `codeterm plugin get <id>` and `codeterm plugin config <id>`. `get` reports each declared secret field as `set` or `unset` and never returns its value. It also returns the plugin's `config_help`, declared `agent_verbs`, and `agent_guide`: `null` when none is declared, otherwise its `path` with either `text` (at most 16 KiB, `truncated` when cut) or an `error` saying why the bundled file could not be read. Use `--set KEY=VALUE` for non-secret fields. For a schema-declared secret field, pipe the credential to `codeterm plugin config <id> --secret FIELD`; this reads stdin, refuses terminal input and empty input, and stores the value in that plugin's secret bucket. `--set` and `--values-json` reject secret fields with a pointer to `--secret`. Keep secret values out of command arguments, `config.yaml`, and logs. `--values-json` writes arrays and objects as nested YAML. A running plugin picks up a `plugin config` change within a few seconds without a reload, and its model list is re-read. `plugin install --from-dir` and `plugin update` keep the plugin's `config.yaml`; grants and all other files come from the new package.

The API listens only on loopback by default. A non-loopback `host` requires a non-empty Bearer token. The existing Mesh switch is the only network-access control: Mesh off rejects every remote request while localhost remains available; Mesh on permits authenticated remote access. The shell's declared frontend origins are allowed to reach its local daemon: the daemon-served frontend, the configured DEV frontend and the platform's packaged fallback. Each `api.cors_allow_origins` entry adds an exact HTTP(S) origin. Wildcards, credentials, paths, query strings and fragments are refused and logged. Other cross-origin clients remain closed. Origins, methods and request headers never use `*`. Authenticated `GET /api/config` redacts the token and reports both the configured address and the listener policy the process actually owns. `localhost`, `::1`, and `[::1]` are canonicalized to bindable loopback socket addresses; other hostnames are rejected in favor of an explicit numeric address.

`codeterm rest-api status` reads listener state through the local socket even when HTTP is down. It reports `phase`, `generation`, `diagnostic` and `message`; a port collision reports `bind_ownership_failed` with the bind address and OS error. Release the foreign port owner, then run `codeterm rest-api restart`. Restart waits for every bound listener's health proof and exits nonzero on failure, including with `--json`. These commands are local-only and never fall back to HTTP. Native Settings → Network also offers Restart REST API. After recovery, use Retry surface lookup on an unavailable Browser surface and confirm the page renders.

Development builds (the default DEV instance or a named one) may explicitly set `api.mesh_tailnet_host` to a locally assigned Tailscale IPv4 address. This adds an authenticated listener while retaining the loopback primary; the default is absent. Production builds refuse this option. A restart is required. Both sockets must bind and pass health checks before remote readiness is published; startup never falls back to a LAN or wildcard bind. If the extra listener exits after readiness, loopback remains serving, remote readiness is withdrawn, and broadcasting stops. `api_listener.ready_addresses` is the live health-proven set, not the configured request; `requested_mesh_tailnet_address` records the startup request only.

Agent access is one global choice. `agent.full_access = true` applies the provider's permission-bypass capability whenever any agent starts or restarts, including General Agent; `false` keeps provider permission prompts everywhere. The first-run picker asks once with Full access selected by default; there are no per-provider, per-resume, audience, or Remember-choice policies. On first load, an enabled remembered bypass from the former global or per-provider settings is converted to `agent.full_access = true` before any agent tab starts; the removed keys are then deleted from the saved config.

With `false`, a role, task, or cron launch still runs its own Domios commands (`agent startup-ack`, `agent ack`, `agent report`, `pad path`, and `cron complete` for a cron run) without an owner prompt: a provider that declares launch permissions grants exactly those commands through launch arguments when the model has no unattended mode, never through global provider settings. An Orchestrator or Planner on such a model is refused before any tab exists, naming the model and the models that have the mode. An Orchestrator launch always loses the provider's native subagent tools.

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

If the config file is held after a failed load, writes fail and leave the live
settings unchanged. Review and repair the salvaged file, remove its CodeTerm
salvage marker, and restart Domios; a known-good config backup can also be
restored before restarting.

## Value limits

Numeric and enum settings are checked on CLI, REST, Tauri, and config-file load. Config strings are limited to 65536 UTF-8 bytes, control characters are refused except line breaks and tabs, collections are limited to 4096 entries, and the full config is limited to 4 MiB. Invalid file values are rejected with a warning and Domios uses the last valid config or defaults.

| Key | Allowed value |
|---|---|
| `font.size` | integer from 8 to 72 |
| `agent.poll_interval_ms` | integer from 100 to 60000 |
| `agent.idle_timeout_s` | integer from 1 to 3600 |
| `memory.workers_per_provider` | integer from 1 to 16; default 3 |
| `supporter.memory_rerank.sources.recall` | integer from 1 to 50; default 30 |
| `supporter.memory_rerank.sources.graph` | integer from 0 to 20; default 3 |
| `supporter.memory_rerank.sources.docs` | integer from 0 to 10; default 3 |
| `supporter.memory_rerank.sources.sessions` | integer from 0 to 5; default 0 |
| `supporter.memory_rerank.max_concurrency` | integer from 1 to 32; default 16 |
| `supporter.calls_per_hour` | optional integer from 0 to 100000; absent or 0 means no cap |
| `supporter.deadline_ms` | integer from 100 to 30000; default 3000 |
| `supporter.memory_rerank.candidates` | integer from 1 to 50; default 50 |
| `supporter.memory_rerank.low_confidence_threshold` | number from 0 to 1; default 0.15 |
| `supporter.memory_rerank.max_body_chars` | integer from 256 to 100000; default 2000 |
| `supporter.memory_rerank.docs_hybrid` | boolean; default false |

`supporter.adapter` is the optional active adapter (absent means stopped); `supporter.connections.<id>.model` and `.endpoint` keep each adapter's settings; `.deadline_ms` sets the stage deadline. `supporter.memory_rerank.docs_hybrid` (default `false`) enables cached vector retrieval for documentation candidates. The active usable adapter automatically serves memory re-rank and tab-state. Switching saved connections keeps their tokens and models; disconnecting active leaves none selected. `supporter.calls_per_hour` optionally caps requests across all tabs (0 to 100000). Absent or 0 means unlimited. Accounting records each physical HTTP transport attempt; local decisions cost no request. Event observations require an adapter that explicitly supports batching and use at most one transport request for up to six named questions. Sequential adapters can spend multiple requests per shared-state search. Status reports requests per hour and day and a null cap when unlimited. `supporter.memory_rerank.request_mode` selects `per_candidate` (retained default until live A/B qualifies fan-out) or `shared_state`; per-candidate mode costs N requests per N-candidate search. Adapters declare token mode: TypeSafe Jev is `required`; plugin manifests use bare `"decisionModel": true` for `optional`, or `{ "tokenMode": "required" | "optional" | "none" }` (the object form requires `minCodeterm: "1.12.4"`). Optional and none adapters connect without a token. Settings > Agents shows request and token counts and estimated spend when prices are declared. HTTP 402 exhaustion produces a failure alert and deterministic fallback. The token is write-only in Settings > Agents > Supporter Agent, or through `codeterm supporter token set` with stdin; it is never stored in `config.toml`.

`memory.dream_hygiene.stale_archive` (default `false`) lets the nightly sweep archive stale items; keep it off until a `codeterm mem calibration stale --dry-run` list has been checked. Per UTC day, `memory.dream_hygiene.stale_archive_cap` (0 to 500, default 20) caps archives and `memory.dream_hygiene.merge_cap` (0 to 100, default 10) caps Dreamer merges.

Other numeric limits and accepted enum values are reported by `codeterm config set <key> <value>` when a value is refused.

## When config.toml fails to load

A `config.toml` that cannot be read or parsed is never treated as empty and never overwritten. The original is renamed to `config.toml.corrupt-<unix-ms>` (newest 5 kept), one ERROR line names the file and the error, and one action-needed notification names the kept copy. Every successful save also writes a rotating backup under `state-backups/config.toml/` (newest 10 kept); the newest backup that parses is restored, so the API token and `agent.full_access` survive. With no valid backup Domios runs on in-memory defaults and refuses every save until the file is fixed or a valid one is put back (picked up without a restart). The API token is never regenerated after a parse error: it and `agent.full_access` are salvaged from the broken file's intact lines, otherwise token-authenticated calls are refused. A save that finds a hand-edited file that no longer parses preserves it the same way first.

## General Agent

The General Agent is an always-on Claude Code instance in the "General" workspace (`[general_agent]` in the reference above). It handles Telegram messages and cron tasks and acts as the default orchestrator. When enabled, Domios starts with a single protected "General" workspace instead of "Main"; its tab cannot be closed from the UI, and Settings > Agents > General Agent toggles it. Recovery thresholds, owner notifications and provider update behaviour: `codeterm docs TAB-LIFECYCLE`, sections "General Agent recovery" and "Provider updates".

## Worker lane provisioning

Provisioning creates an isolated git worktree and launches the agent there; it installs no dependencies and links nothing from the canonical checkout (`node_modules` is never shared by symlink or junction, and no package-manager command precedes the agent command). An isolated check that needs dependencies installs them inside its own lane; full rebuilds stay in canonical.

## Codex structured control

Codex structured control is enabled by default in 1.12.4. The provider declares the `codex-app-server` capability with minimum version `0.159.0`; `[agents.codex] structured_control_enabled = false` explicitly selects the plain PTY lane. Each structured tab owns its app-server, port and authentication token. The server owns session binding, thread identity and busy/idle state; provider hooks are telemetry only for those tabs. PTY fallback tabs retain ordinary hook behavior. Unsupported versions, failed readiness, unknown extra terminal arguments and Windows `.cmd`/`.bat` wrappers automatically select the plain PTY fallback. Launch diagnostics record the reason. No token value appears in launch arguments or diagnostics.

## Related documents

Notification settings: `codeterm docs NOTIFICATIONS`. Integration settings: `codeterm docs INTEGRATIONS`. Runtime, UI, and memory settings: `codeterm docs RUNTIME-CONFIG`. Provider CLI updates run silently on the production instance when its provider has no active turn. Running agents are never restarted; the new CLI applies at their next launch. Results appear in logs and Settings. Disable automatic updates with `updater.auto_update_provider_clis = false` (default: true). Named and non-production instances join only with `updater.allow_provider_updates_in_non_prod = true`. Declared-incompatible targets are withheld; an updater with declared breaking ranges must pin a supported target.
Utility limits apply to each Team and purpose (`build`, `dev`, `qa`, `deploy`, `logs`, `db`, `shell`). At the limit, creation restarts the oldest confirmed idle tab with the new command and description; busy or unknown tabs are refused. Use `codeterm tab new --description "Runs focused tests" --purpose qa --icon bug`; icon names are Lucide kebab-case names. Structured provider access refusals are always observed from the tab's bound session, independently of optional structured-signal settings. Incident levels and recovery are in the notification document above. Capability plugins may declare `permissions.projectFiles` as config selectors such as `["/root", "/profiles/*/root"]`. These configured, existing absolute directories become read roots. They remain read-only until the owner explicitly approves `codeterm plugin config <id> --grant project_write`. Approval records canonical roots in `grants.yaml`; changing a profile root never transfers that write grant. Revoke with `--revoke project_write`. Reload the plugin after root or grant changes. Symlink roots, traversal and symlink escapes are refused, including writes below missing parents. Project-file plugins use the confined filesystem even in DEV; they cannot rewrite their own `grants.yaml`. A separately granted arbitrary subprocess remains outside the file-tool confinement contract.

Domios account sign-in and product analytics settings: `codeterm docs ACCOUNT`.

<!-- revision: 113 -->
