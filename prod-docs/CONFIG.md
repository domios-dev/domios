# Domios Configuration

Config file: `~/.domios/config.toml` on every platform, auto-created with defaults on first run. `domios identify` shows the active data directory. `[tab_sleep]` has `enabled = true` and `idle_minutes = 60` (15..=10080); idle tabs resume their exact session on interaction.

Sensitive configuration and state use private file writes: Unix files use `0600`; Windows removes inherited and broad access while preserving explicit narrow grants. Data-root, CLI, agent-pad and browser permissions are retained, and shell umasks are unchanged. Startup protects `config.toml` and `.secrets` directly; named state files are tightened in a bounded background pass after the early listener starts. Unrelated state-file errors are logged, never fatal. The instance API credential is stored at `<data_dir>/.secrets` under `domios.instance:api_token`, read once at daemon startup and retained across configuration reloads. DEV (`~/.domios-dev/`, or `~/.domios-dev-<instance>/`) and production (`~/.domios/`) keep separate credentials. An existing `[api].token` is rotated once on startup: the replacement persists before the legacy entry leaves `config.toml`. Existing backups and corrupt copies are retained and their old bearer is refused. A failed rotation preserves the original configuration and active credential, holds config writes until restart, and reports the error.

`domios system api-token` prints only the selected local host credential, without a newline or a daemon connection. Capture it privately; tab callers and `--mesh` are refused. Tabs receive distinct launch-scoped credentials through `CODETERM_API_TOKEN`; they never receive or fall back to the host credential.

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
# [notifications]: levels, surfaces, sound and push — domios docs NOTIFICATIONS
[orchestration]
plan_reminders = true
nudge_interval_secs = 2700  # 45 minutes
# digests.rules: in_review_gt, jobs_gt, agent_tabs_gt, idle_utility_shells_gt, finished_lanes_gt cleanup rules default to threshold 10, rate_limit_secs 3600.
# Templates accept {count}, {in_review}, {jobs}, {agent_tabs}, {idle_utility_shells}, {idle_workers}, {finished_lanes}, {lane_candidates}, {plan_id}.
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
# sandbox_ceiling: optional "read-only" or "workspace-write" cap for delegated spawns.
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true
continue_after_restart = true  # false disables it (domios docs TAB-LIFECYCLE)
full_access = true

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
# The API credential is managed in .secrets, outside configuration.

[general_agent]
capability_hints = true       # Relevant feature hints during the first week
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

Toggle it live with `domios config set diagnostics.terminal_transport true|false`. The next output-stream session and PTY write use the new setting; an open stream keeps the setting it started with. For mesh traces, enable it on both the local relaying daemon and the peer. No restart is required.

## Protocol recording

`diagnostics.protocol_recording` is off by default and only records in a named development instance. When enabled, resolved hook posts are written as redacted NDJSON under `logs/protocol/<tab>/`. A single background writer keeps at most ten 10 MiB files per tab, 512 MiB total, and 14 days of history. A full writer queue drops the record and increments the recorder's dropped counter; hook responses never wait for disk writes. Toggle it live with `domios config set diagnostics.protocol_recording true|false`.

`kpi.enabled` owns the ledger behind Mission Control's Tokens and Turns cards. It records plan, task and tab ids, event kinds, byte counts, token counts and timestamps in `~/.domios/kpi_ledger.db` — never message text, and nothing leaves the machine. Opt out with `domios config set kpi.enabled false`; opt back in with `domios config set kpi.enabled true`. A config saved before KPI defaulted on still carries the old `enabled = false`; the first load after upgrading switches it on once and records `kpi.default_applied = true`, so an opt-out saved afterwards is kept. Either way the change takes effect on the next Domios start, and while it is off the Board shows an explicit not-recording state naming this key rather than a zero.

Stored model defaults stay on models their provider still offers. These are `agents.<id>.default_model`, `memory.extraction_model`, `memory.dreaming_model`, `memory.worker_model` and each scheduled job's `model`. At startup and on every model catalogue change, the daemon checks each default against its provider's catalogue, using the same matching as spawn. For a provider with an authoritative catalogue, a default it no longer offers is dead. A free-form catalogue (one that accepts unlisted ids) only counts a default as dead once it stops listing a model it listed earlier. A provider that needs live discovery is checked only against a live snapshot. A dead default moves to the newest model in the same family; failing that, to the provider's recommended model; failing that, to the first model it offers. One notification lists every change. `model_defaults.replaced.<setting path>` records `from`, `to` and the reason. Settings → Agents → Model defaults shows "Updated automatically from …" while the replacement is still the configured value. Supporter decision-adapter models are not checked: their adapters publish no authoritative catalogue and no change event.

## Plugin settings

Plugin-defined settings live under `config.plugins.<id>` and are preserved as a plugin-specific settings bag. The Extensions list and titlebar Plugins button open installed plugin pages; view plugins render their embedded page there.

Agent-facing plugin settings use `domios plugin get <id>` and `domios plugin config <id>`. `get` reports each declared secret field as `set` or `unset` and never returns its value. It also returns the plugin's `config_help`, declared `agent_verbs`, and `agent_guide`: `null` when none is declared, otherwise its `path` with either `text` (at most 16 KiB, `truncated` when cut) or an `error` saying why the bundled file could not be read. Use `--set KEY=VALUE` for non-secret fields. For a schema-declared secret field, pipe the credential to `domios plugin config <id> --secret FIELD`; this reads stdin, refuses terminal input and empty input, and stores the value in that plugin's secret bucket. `--set` and `--values-json` reject secret fields with a pointer to `--secret`. Keep secret values out of command arguments, `config.yaml`, and logs. `--values-json` writes arrays and objects as nested YAML. A running plugin picks up a `plugin config` change within a few seconds without a reload, and its model list is re-read. `plugin install --from-dir` and `plugin update` keep the plugin's `config.yaml`; grants and all other files come from the new package.

The API listens only on loopback by default. A non-loopback `host` requires a non-empty Bearer token. The existing Mesh switch is the only network-access control: Mesh off rejects every remote request while localhost remains available; Mesh on permits authenticated remote access. The shell's declared frontend origins are allowed to reach its local daemon: the daemon-served frontend, the configured DEV frontend and the platform's packaged fallback. Each `api.cors_allow_origins` entry adds an exact HTTP(S) origin. Wildcards, credentials, paths, query strings and fragments are refused and logged. Other cross-origin clients remain closed. Origins, methods and request headers never use `*`. Authenticated `GET /api/config` redacts the token and reports both the configured address and the listener policy the process actually owns. `localhost`, `::1`, and `[::1]` are canonicalized to bindable loopback socket addresses; other hostnames are rejected in favor of an explicit numeric address.

`domios rest-api status` reads listener state through the local socket even when HTTP is down. It reports `phase`, `generation`, `diagnostic` and `message`; a port collision reports `bind_ownership_failed` with the bind address and OS error. Release the foreign port owner, then run `domios rest-api restart`. Restart waits for every bound listener's health proof and exits nonzero on failure, including with `--json`. These commands are local-only and never fall back to HTTP. Native Settings → Network also offers Restart REST API. After recovery, use Retry surface lookup on an unavailable Browser surface and confirm the page renders.

Agent access is one global choice. `agent.full_access = true` applies the provider's permission-bypass capability whenever any agent starts or restarts, including General Agent; `false` keeps provider permission prompts everywhere. The first-run picker asks once with Full access selected by default; there are no per-provider, per-resume, audience, or Remember-choice policies. On first load, an enabled remembered bypass from the former global or per-provider settings is converted to `agent.full_access = true` before any agent tab starts; the removed keys are then deleted from the saved config.

`agent.sandbox_ceiling` is unset by default, so Workers keep full machine access. Set to `read-only` or `workspace-write`, every delegated spawn must pass `--sandbox` at or below it; an omitted `--sandbox` means full access and is refused, as is any other value. Tabs restore and restart with the sandbox recorded at their launch.

With `false`, a role, task, or cron launch still runs its own Domios commands (`agent startup-ack`, `agent ack`, `agent report`, `pad path`, and `cron complete` for a cron run) without an owner prompt: a provider that declares launch permissions grants exactly those commands through launch arguments when the model has no unattended mode, never through global provider settings. An Orchestrator or Planner on such a model is refused before any tab exists, naming the model and the models that have the mode. An Orchestrator launch always loses the provider's native subagent tools.

## Config API

Read and write config values at runtime via REST or CLI.

REST uses authenticated `GET /api/config?key=<dot-path>` and `PATCH /api/config` with `{"key":"<dot-path>","value":<JSON>}`. Omit `key` to read the full redacted config. See `domios docs API`.

**CLI:**
```bash
domios config get integrations.telegram_enabled
domios config set integrations.telegram_enabled true
domios config get general_agent              # read an entire section
domios config get                            # no key → dump the whole redacted config
```

`domios config get` with no key omits the `?key=` query and prints every key/value (secrets redacted). Passing a key scopes the read to that dot-path.

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

`supporter.adapter` is the optional active adapter (absent means stopped); `supporter.connections.<id>.model` and `.endpoint` keep each adapter's settings; `.deadline_ms` sets the stage deadline. `supporter.memory_rerank.docs_hybrid` (default `false`) enables cached vector retrieval for documentation candidates. The active usable adapter automatically serves memory re-rank and tab-state. Switching saved connections keeps their tokens and models; disconnecting active leaves none selected. `supporter.calls_per_hour` optionally caps requests across all tabs (0 to 100000). Absent or 0 means unlimited. Accounting records each physical HTTP transport attempt; local decisions cost no request. Event observations require an adapter that explicitly supports batching and use at most one transport request for up to six named questions. Sequential adapters can spend multiple requests per shared-state search. Status reports requests per hour and day and a null cap when unlimited. `supporter.memory_rerank.request_mode` selects `per_candidate` (retained default until live A/B qualifies fan-out) or `shared_state`; per-candidate mode costs N requests per N-candidate search. Adapters declare token mode: TypeSafe Jev is `required`; plugin manifests use bare `"decisionModel": true` for `optional`, or `{ "tokenMode": "required" | "optional" | "none" }` (the object form requires `minCodeterm: "1.12.4"`). Optional and none adapters connect without a token. Settings > Agents shows request and token counts and estimated spend when prices are declared. HTTP 402 exhaustion produces a failure alert and deterministic fallback. The token is write-only in Settings > Agents > Supporter Agent, or through `domios supporter token set` with stdin; it is never stored in `config.toml`.

`[memory.embedder]` selects the one memory embedder: `provider` (default `local`), `model` (empty means the provider's default), `dimension` (0 means the model's own) and `remote_opt_in` (default `false`). Switch with `domios mem embedder use`, which re-embeds in the background; editing these keys directly takes effect at the next daemon start. A remote provider also needs its stored key; see the Embedder section of `domios docs MEMORY`.

`memory.dream_hygiene.stale_archive` (default `false`) lets the nightly sweep archive stale items; keep it off until a `domios mem calibration stale --dry-run` list has been checked. Per UTC day, `memory.dream_hygiene.stale_archive_cap` (0 to 500, default 20) caps archives and `memory.dream_hygiene.merge_cap` (0 to 100, default 10) caps Dreamer merges.

Other numeric limits and accepted enum values are reported by `domios config set <key> <value>` when a value is refused.

## When config.toml fails to load

A `config.toml` that cannot be read or parsed is never treated as empty and never overwritten. The original is renamed to `config.toml.corrupt-<unix-ms>` (newest 5 kept), one ERROR line names the file and the error, and one action-needed notification names the kept copy. Every successful save also writes a rotating backup under `state-backups/config.toml/` (newest 10 kept); the newest backup that parses is restored, so `agent.full_access` and other settings survive. With no valid backup Domios runs on in-memory defaults and refuses every save until the file is fixed or a valid one is put back (picked up without a restart). The API credential comes from the separate local secret store and is never regenerated after a parse error. During the one-time migration an intact legacy token can be salvaged and rotated; otherwise a missing credential refuses token-authenticated calls. `agent.full_access` is salvaged from intact lines. A save that finds a hand-edited file that no longer parses preserves it the same way first.

## General Agent

The protected General workspace starts when `general_agent.enabled` is true. Settings > Agents > General Agent toggles it; its tab cannot be closed from the UI. It handles Telegram and cron work. Recovery and provider updates: `domios docs TAB-LIFECYCLE`.

`general_agent.capability_hints` defaults to `true`; Settings calls it “Suggest Domios features I have when relevant”. For seven days after this machine's first General session, its prompt includes a bounded installed capability list. Suggestions must help the request and appear at most once per topic. Disabling removes the fragment at the next composition; first-seen time never resets.

## Worker lane provisioning

Provisioning creates an isolated git worktree and launches the agent there; it installs no dependencies and links nothing from the canonical checkout (`node_modules` is never shared by symlink or junction, and no package-manager command precedes the agent command). An isolated check that needs dependencies installs them inside its own lane; full rebuilds stay in canonical.

## Codex Chat runtime

Codex uses Chat through provider-codex's `agentRuntime`: a tab-owned `codex app-server` stdio JSON-RPC process. The plugin owns protocol; Domios owns lifetime, session binding and delivery. There is no terminal, remote attachment, listener or token file; raw keys and resize are unsupported. `structured_control_enabled` is removed.

Exit or pipe failure keeps the saved thread and displays the reason, exit status and bounded stderr. Readiness, send, command or Retry initializes and resumes within a deadline. History reconciliation never replays uncertain input or the launch brief. A missing thread is reported; there is no PTY fallback or timer retry. Stop interrupts the turn and keeps the runtime available. This applies to General, project, mesh and restored tabs across restarts and provider updates.

## Related documents

Notification settings: `domios docs NOTIFICATIONS`. Integration settings: `domios docs INTEGRATIONS`. Runtime, UI, and memory settings: `domios docs RUNTIME-CONFIG`. Provider CLI updates run silently on the production instance when its provider has no active turn. Running agents are never restarted; the new CLI applies at their next launch. Results appear in logs and Settings. Disable automatic updates with `updater.auto_update_provider_clis = false` (default: true). Named and non-production instances join only with `updater.allow_provider_updates_in_non_prod = true`. Declared-incompatible targets are withheld; an updater with declared breaking ranges must pin a supported target.
Utility limits apply to each Team and purpose (`build`, `dev`, `qa`, `deploy`, `logs`, `db`, `shell`). At the limit, creation restarts the oldest confirmed idle tab with the new command and description; busy or unknown tabs are refused. Use `domios tab new --description "Runs focused tests" --purpose qa --icon bug`; icon names are Lucide kebab-case names. Structured provider access refusals are always observed from the tab's bound session, independently of optional structured-signal settings. Incident levels and recovery are in the notification document above. Capability plugins may declare `permissions.projectFiles` as config selectors such as `["/root", "/profiles/*/root"]`. These configured, existing absolute directories become read roots. They remain read-only until the owner explicitly approves `domios plugin config <id> --grant project_write`. Approval records canonical roots in `grants.yaml`; changing a profile root never transfers that write grant. Revoke with `--revoke project_write`. Reload the plugin after root or grant changes. Symlink roots, traversal and symlink escapes are refused, including writes below missing parents. Project-file plugins use the confined filesystem even in DEV; they cannot rewrite their own `grants.yaml`. A separately granted arbitrary subprocess remains outside the file-tool confinement contract.
Domios account sign-in and product analytics settings: `domios docs ACCOUNT`.

<!-- revision: 127 -->
