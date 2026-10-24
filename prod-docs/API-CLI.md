# CLI and environment API

Immediate `domios send` exits 1 with its reason on stderr whenever the result is `ok:false` or `bytes_written:false`, including retained pre-ready input. With `--json`, the error envelope preserves the daemon receipt in `data`. A retained `send_id` or queued delivery requires status or exact own-session transcript inspection; never replay held, written or uncertain input.

Cron JSON exposes `protected: true` for system jobs. System jobs refuse deletion and schedule/task edits; use `domios cron enable <id>` or `domios cron disable <id>` to control them.


## CLI

Use `domios <command> --help` for the installed CLI contract. `codeterm` remains a fully working alias; both names show canonical `domios` help and usage.

### Startup readiness

With the socket unavailable, credentialed CLI fallback probes RPC directly. A `starting` reply waits on startup events for up to 30 seconds before repeating the identify probe and submitting the first command. Without credentials, discovery checks the selected instance's loopback health first. REST recovery refuses immediately. Errors name the phase; JSON codes `starting`/`startup_failed` preserve `data.startup`. Fallback uses the selected local secret-store credential when absent from the environment. Commands never replay.

### Remote task locations

`agent providers --json` exposes an optional `remote_task` capability on the provider itself. Its `mode` identifies an explicit execution location; `availability` and `environments` determine whether Cloud can start. A declared location does not establish live account access.

```bash
domios agent spawn <provider> --location cloud --environment <environment-id> --description "Remote task"
```

Choose an environment explicitly; an unambiguous label is also accepted. Local remains the default. Cloud opens a one-task tab, with a branch and prompt chosen there, or `--task` supplies its first task on `master`. Local model controls, roles and Worker bindings are refused for remote tasks. Run this command on the machine that owns the provider; remote task spawning through `--mesh` is unavailable.

The task tab shows persisted results/diffs and offers Refresh. Restore uses the existing task identity without creating another task. Unconfirmed creation must be checked in the provider's task list before another task is submitted. Unsupported follow-up, cancellation and patch application are never substituted with terminal commands or another billed backend. An empty environment list asks you to connect the owning account and refresh providers.

Grant outbound hosts with `domios plugin settings <id> --allow-host <host[:port]>`; revoke with `--disallow-host`. Grants combine with manifest `permissions.network.allow` on plugin load. Bare hosts cover any port; host:port restricts it. Matching is exact: no schemes, paths, credentials, wildcards or subdomains.

Plugin verbs accept `--from-pad PATH`, `--from-file PATH` or `--stdin` before `--` (`-` reads stdin). Text reaches `ctx.body` exactly, separate from args. Pad containment applies. The body limit is 1 MiB of UTF-8 bytes; the socket refuses larger bodies with `plugin_body_too_large`. Before `--`, these flags select the host body source. Pass plugin-owned `--from-file`/`--stdin` after `--`; args preserve the separator, e.g. `domios plugin <id> <verb> -- --from-file input.json`.

### Provider plugins and their CLIs

`domios plugin install` and `domios plugin update` report each provider's CLI status from the installed manifest. Missing CLIs produce a warning with the exact installer for this OS and the management verb. Plugin installation and CLI installation are separate operations; chat backends require no CLI. When connected to the daemon, plugin install/update waits for live provider registration acknowledgement before returning. JSON `provider_registration` is `acknowledged`, or `offline` when no daemon connection is available. An offline install takes effect when the daemon loads plugins.

```bash
domios agent cli status <provider>
domios agent cli install <provider>
domios agent cli update <provider>
```

The `agent cli` verbs are local-only; run them in a tab on the target peer. Status returns `required`, `installed` (true/false/null for unknown), `binary`, `resolved_path`, `detected_version`, `latest_version`, `update_job`, `update_refusal`, `availability`, `searched`, `installer`, and `updater`. `latest_version` is the published update availability; status never starts discovery. `update_job` reports the current job state/result. Status also includes `installer_prerequisites` and `updater_prerequisites`: detected command runners with `binary`, `installed` (true/false/null), and a setup `hint`. Runners and `command -v`/`which`/`Get-Command` checks are detected from the manifest command (diagnostic, not a shell parser); missing runners produce warnings and setup guidance. The host never installs prerequisites or alters manifest commands. Dispatch JSON adds `warnings`; completion retains prerequisite diagnostics. Install/update refuse when the OS has no manifest command; otherwise they open a visible utility tab in the caller's workspace and Team and return `tab_id`, `launch_state: "command_dispatched"`. Shell completion or exit produces a routed report for an agent caller or a kept notification record for a shell caller. Completion prose is never sent as shell input; the result also lands in the utility tab description and `provider-cli-completed` carries a fresh status. A completion deadline reports uncertainty; dispatch alone never means installation succeeded. `agent providers` names missing CLIs; `agent spawn` refuses before creating a tab and names the installer and CLI fix. REST: `GET /api/agents/providers/{id}/cli` returns status; `POST` accepts `{operation: "install"|"update", caller_tab_id?}`. Plugin `--json` responses add `cli_status`, keyed by installed provider id.

### Agent usage, reset credits, and child history

```bash
domios agent usage read --tab TAB_ID [--json]
domios agent usage reset --tab TAB_ID --confirm [--json]
domios agent child-history --tab PARENT_TAB --thread CHILD_THREAD --root-thread-id ROOT_THREAD --binding-nonce NONCE --epoch EPOCH [--cursor CURSOR] [--json]
```

These commands always name the target tab. `usage read` returns session/weekly limits, account credits, reset-credit count and expiry from that tab's cached runtime projection. `usage reset` consumes a reset credit through the existing runtime route; `--confirm` is mandatory and each invocation carries a fresh idempotency UUID. Its JSON includes the provider outcome and observed before/after reset-credit counts. `child-history` passes the current parent binding authority to the existing recorded-child route; stale bindings and unrecorded descendants are refused. The cursor is opaque and may be supplied from `nextCursor` to continue.

Both read `GET /api/tabs/{id}/agent/runtime/usage` (exact-tab, binding-checked projection); a missing post-consume count stays `null`.

`domios agent model --tab TAB_ID [MODEL]` switches the model; `--reasoning-effort` and `--runtime-mode` take catalogue values (omit `MODEL` to keep it). Unsupported values exit non-zero listing the choices; `--json` keeps `mode_used`, `model_confirmation`, `evidence`. `--speed TIER|default` and `--list-speeds` manage service tiers. Worker kinds work with or without `--plan-task`:

```bash
domios agent spawn PROVIDER --role Worker --kind qa --parent-tab TAB_ID
domios agent spawn PROVIDER --role Worker --kind release_manager --parent-tab TAB_ID
domios agent spawn PROVIDER --role Worker --kind reviewer --parent-tab TAB_ID --task "exact review scope"
```

Message and pad file options accept `-` as UTF-8 stdin, including `plan task remote-report --message-file -`, `send --from-file -`, and `--from-pad -`. For `plan create`, an implicit project path from a Team-bound caller is the caller Team workspace cwd; explicit `--project` remains an override.

### Plugin update diagnostics

`domios plugin list` shows `update=<reason>` and `channel_version=<version>`. With `--json`, each installed row includes `update.availability` (the shared item below), `update.available`, `update.reason`, `update.channel`, `update.remote_version`, and `update.basis` (`channel_cache`). Listing is offline: refresh a named channel with `domios plugin channel refresh <name>` to check its current offer. `domios plugin update <id>` refreshes the recorded channel and installs only a newer compatible semantic version whose channel and plugin manifests agree. Bulk update skips plugins that are not updatable.

Same-version manifest hash drift remains a diagnostic in `plugin channel diff`; it does not produce an UPDATE badge or count. Local directory installs and source tree plugins have no channel update. Reinstall local changes with `domios plugin install --from-dir <path>`. Unknown install provenance or a missing channel cannot authorize an update. `plugin update-system` remains the explicit system synchronization and same-version repair operation; it preserves newer installed versions.

Official system plugins have independent release versions. Their `minCodeterm` declares the required host version, so a compatible plugin can update without updating the app. The embedded copy is an offline fallback and never replaces a newer channel release. Use `domios plugin update-system <id>` to install an official system release even when an embedded install has no channel provenance yet. Core plugins synchronize silently on startup and explicit plugin channel refresh; they are always `core_silent` and never offer a user Update action. Other channel plugin updates remain explicit.

`domios update check [--force]` returns `{items, errors, checked_at_ms}` from `GET /api/update/availability?force=true` (omit force to reuse a snapshot younger than 30 seconds with background refresh; missing or older snapshots wait for discovery). The desktop command is `check_update_availability`. Each item has `{id, kind: plugin|app|frontend, installed, latest, compatible, state: up_to_date|update_available|ready_to_apply|incompatible|core_silent, reason, channel}`; versions and channel may be null. Errors are keyed by source. `checked_at_ms` is the discovery start time in Unix milliseconds. Checks refresh channel metadata without installing or reloading provider plugins. `ready_to_apply` identifies a verified staged Interface bundle, offered even when no downloaded bundle is active. Apply and Update all use the existing checkpoint, consent, activation and health/rollback flow. The app offer comes from the platform's updater manifest; frontend offers come from signed frontend-latest metadata and its host API range. A plugin offer must come from its recorded channel, match its payload version, and meet both minimum host declarations. `up_to_date` with an unavailable or malformed source reason does not authorize installation.

`update-availability-changed` carries the same `{items, errors, checked_at_ms}` snapshot when discovery or completed plugin actions change it. Plugin enable/disable settings recompute the inventory too. `plugin channel diff --json` includes the same item as `availability` on each row; refresh the channel first for fresh metadata. Aggregate refresh reports failures after trying all channels and preserves their last validated caches. A channel failure names its cause and source, for example `Plugin channel unavailable: repository not found or not readable without access (github.com/owner/repo.git). Domios will retry.`; causes are repository not found, authentication required, network unavailable, release asset not found, access denied, and failed integrity check. The official `codeterm-core` channel is a public release archive with a SHA-256 sidecar, readable without a GitHub identity. When it is unreachable the bundled core set stays installed, core items stay `core_silent` against the bundled manifest, and its error is diagnostic only.

### CLI JSON ID Contract

Planner mutation commands emit one JSON envelope: `{ok, kind, id, data, hints?, error?}`. `id` is the primary created or mutated entity id, and `data` contains the raw daemon payload with typed fields such as `plan_id`, `task_id`, `spec_id`, and `section_id`. For example, `spec create --json` returns `kind == "spec"` and `id == spec_id`, `plan create --json` returns `kind == "plan"` and `id == plan_id`, and `plan task add --json` returns `kind == "task"` and `id == task_id`.

`plan task dispatch <plan> <task> --mesh <peer-instance-id> --execution-intent mutating` starts origin-owned remote work. The Worker ACKs and reports to that attempt; after the Orchestrator accepts the report, run `plan task merge-remote-lane <plan> <task> <attempt>` on the origin to fetch the authenticated peer lane at the accepted report SHA and create a real merge commit into the clean dispatch-time branch.

### CLI JSON Errors

With `--json` commands print exactly one JSON object on stdout for success and for failure, including argument errors; stderr stays empty. The local-only `domios system api-token` success output is an exception: it always prints only the credential, without a newline or JSON wrapper. A failure is `{"ok":false,"kind":"error","id":null,"data":...,"hints":[],"error":{"code","message","hint","rpc_code"}}` and the command exits non-zero. `error.code` is one of `not_found`, `invalid_argument`, `conflict`, `rate_limited`, `refused`, `unavailable`, `internal`, or a command-specific snake_case code; a missing tab, plan, cron job, peer, or plugin is always `not_found`. `rpc_code` is the raw daemon code or null. Exit codes: `0` success, `1` failure, `2` invalid arguments, and the command-specific codes named in `SECRETS` and `MESH-BRIDGE`. Structured daemon error data stays under `data`, including `invalid_model` fields such as the provider, requested id, closest catalogue ids, and refresh command. An ambiguous bare id uses `ambiguous_model` with the provider choices and a message asking for `--agent`. Mesh relay failures keep the peer's own delivery `code` inside `error`.

### Sandboxed CLI transport

The CLI tries its local pipe/socket first, then authenticated loopback `POST /api/cli/rpc` for socket and REST-backed commands. Credentials resolve through one local secret-store boundary: `CODETERM_API_TOKEN` for a matching tab connection, otherwise the selected instance's `<data_dir>/.secrets`. An explicit DEV selection needs a configured or explicit port before its callback origin can match an inherited token; foreign selections never reuse a production token. Both startup discovery and RPC fallback refuse a callback URL differing from the resolved instance address.

HTTP fallback requires exactly `http://127.0.0.1:<port>` without credentials, path, query or fragment; proxies and redirects are disabled. Named custom-port targets also need `CODETERM_INSTANCE` or an explicit socket path. It dispatches the same socket methods, including `api.request` for REST-backed commands. The credential must be the local API token and the TCP peer loopback; mesh peer tokens are refused. Success wraps data in `{result: ...}`; an inner `error` remains ordinary data.

### Mesh CLI failures

`domios mesh link <installation-id>` checks the HTTP result before printing success. A refusal exits non-zero and carries the typed `failure.reason`; `mesh.machines` means the account's free machine slots are used.

`domios tab list --mesh <peer>` exits non-zero when the peer has no online mesh route; `(no tabs)` is reserved for a successful response from an online peer. `domios tab get-text --mesh <peer>` requires a PTY-backed remote tab. If the tab has no PTY, the CLI reports that condition and suggests choosing a PTY-backed tab.

Use the CLI or documented REST routes; desktop-only controls are available through the desktop UI.

## Environment Variables

Set automatically in every PTY spawned by Domios:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Running inside Domios |
| `CODETERM_VERSION` | `0.4.0` | App version |
| `CODETERM_SOCKET_PATH` | `\\.\pipe\codeterm-1234` | Explicit socket path; it wins over socket derivation. Named dev sockets also select their matching data directory. |
| `CODETERM_TAB_ID` | `a3f1b2c7` | Current tab ID (8-char hex) |
| `CODETERM_WORKSPACE_ID` | `b4e2c8d1` | Current workspace ID (8-char hex) |
| `CODETERM_ROLE` | `Manager` | Structural agent role: `Manager` or `Worker` (empty if no role). The finer kind (`orchestrator`, `planner`, `watcher`, …) lives in `role_profile`, not here; display surfaces render that kind's label. Set at PTY spawn. |
| `CODETERM_PARENT_TAB_ID` | `b4e2c8d1` | Parent Tab ID in hierarchy (empty for a Team anchor or no role). Set at PTY spawn. |
| `CODETERM_API_PORT` | `7685` | API port for the running instance. Set by Domios so CLI and hooks use the correct port. A custom port needs `CODETERM_INSTANCE` or an explicit matching `CODETERM_SOCKET_PATH`. |
| `CODETERM_API_URL` | `http://127.0.0.1:7685` | Optional API origin for local CLI fallback; fallback accepts only the exact IPv4 loopback host. |
| `CODETERM_API_TOKEN` | `your-secret-token` | Bearer token for the API. Injected into every PTY when `[api].token` is set, so CLI subcommands and the auth-enforced hook callbacks (`/api/tabs/*/hook/*`) can authenticate without per-call wiring. |
| `TERM` | `xterm-256color` | Terminal type |
| `COLORTERM` | `truecolor` | Color support |
| `TERM_PROGRAM` | `CodeTerm` | Terminal name |

## Agent Integration

When a readiness report is refused locally, `domios agent report` prints `REPORT NOT SENT` and sends a diagnostic to the parent. In JSON output, `refused_report_diagnostic_sent`, `diagnostic_target_tab_id`, and `diagnostic_error` describe that diagnostic. The former `diagnostic_report_sent` and `diagnostic_report_id` keys and `report_outcome` field are not emitted. A refused report remains absent from task history and does not change task status.

Domios automatically detects Claude Code and other agents:
- **Permission requests** → in-app toast + Windows notification + webhook
- **Task completion** → in-app toast + Windows notification + webhook
- **Agent icon** on tab (orange ✻ for Claude Code)
- **Pinned tab names** — manual rename blocks OSC auto-rename

Skills for Claude Code are in `skills/` directory. Install via Settings → Agents & Skills.

`domios agent models <provider> --json` returns that provider's model rows,
including over `--mesh <peer>`; an unknown provider returns a typed `not_found`
error. `domios agent providers` lists provider summaries and returns the full
descriptor array with `--json` in both local and mesh mode.

`domios agent models <provider>` refreshes required live catalogues; if no snapshot publishes, it warns and lists the registered one. Spawn with `domios agent spawn [provider] --model <id>`; without a provider, a uniquely advertised ID selects its provider, otherwise the configured default applies. Authoritative catalogues refuse unknown IDs and list valid ones. Free-form providers validate published live IDs, passing IDs unchanged if no list exists. Qualified IDs select their provider. Ambiguous IDs require `--agent` on dispatch or the positional spawn provider; foreign aliases refuse rather than cross providers.

Switch a tab's runtime service tier with `domios agent model --tab TAB --speed TIER`; use `--speed default` to clear it or `--list-speeds` to read the exact tab's session-bound, model-scoped catalogue. `GET /api/tabs/{id}/agent/service-tiers` returns available tiers and requested/confirmed selection. Unavailable tiers are refused before the existing model-control endpoint is called.

Task attempt inspection keeps `requested_model`, `provider_resolved_model`, and `session_model` separate. The resolved field stays null until launch resolution identifies the model; the session field stays null until detection reads that attempt's bound session. A different detected model is stored in `model_identity_mismatch` and raises one dispatcher notice.

### Open agent shells (`chatBackend`)

Beyond PTY agents, a tab can host an **open agent shell** — a non-PTY chat tab driven by a `chatBackend` plugin that talks to a local LLM, runs its own ReAct tool loop inside the plugin sandbox, and renders its active system prompt as an observable first-message card. Shells are discovered and spawned through the same surfaces as agents:

- **Discover:** `GET /api/agents/providers` (CLI `domios agent providers`) lists installed shells alongside PTY agents, tagged `transport: "chat_backend"`. There is no hardcoded shell list — core enumerates the `chatBackend` registry. Every row carries `availability` (`available` / `unavailable` / `unknown`); the CLI marks the latter two inline, and the UI provider picker reads the same field. `cli_status.detected_version`, `version_probe`, and `compatibility` expose the cached CLI version and declared compatibility verdict with typed reasons. The first observation can be `pending` with an unknown verdict while a bounded worker probes the binary. Unknown versions never authorize a declared direct sqlite reader.
- **Spawn:** `domios agent spawn <shell-id> [--system TEXT] [--preset ID] [--model M]` (or `POST /api/agents/spawn`) routes by capability to the shell. `--system` overrides the preset's system prompt for that session, `--preset` picks a preset, no flag → the plugin's `defaultPreset`. These and the watcher flags are refused for a PTY agent, which has no use for them.
- **Watcher spawn:** `domios agent spawn <shell-id> --role Watcher --parent-tab <orchestrator-tab> --charter <TEXT|charter:id> [--interval 30s] [--nudge-cooldown 10m] [--max-actions 3]`. `--charter-file PATH` may be used instead of `--charter`. The REST body fields are `charter`, `tick_interval_secs`, `nudge_cooldown_secs`, and `max_actions_per_tick`. The host opens the shell with `mode: "watcher"` and `engine: { kind: "machine", charter, tickIntervalSecs }`, rejects direct sends into the watcher tab, schedules `watcherTick`, parses `watcher_verdict` messages, and executes only host-allowlisted verdict actions.
- **Open directly:** `POST /api/tabs {chat_backend, system_prompt?, preset?, model?}`.
- **Observe:** the shell emits a `system_prompt` marker message rendered as a collapsible "System prompt" card; tool-call / tool-result messages render via the existing chat-UI message rendering.



## Command Palette

Command-palette shortcuts and agent-control workflows: `domios docs AGENT-CONTROL`.

## System Tray

Domios hides to the system tray on window close instead of quitting. To fully quit, use the tray menu. This ensures cron jobs and background agents continue running when the window is closed.

## Role presets

A launch's role text comes from its role preset; task personality, `role_guidance` and `executor_role` enter as its task overlay.

### Role preset inspection

| Command | RPC | Purpose |
|---|---|---|
| `domios persona list` | `persona.list` | Every reachable role kind with capability columns, plus refused overlays and why |
| `domios persona show --role <Manager\|Worker> [--kind <id>] [--guidance <text>] [--occasion <id>]` | `persona.show` | The exact text each occasion emits, its fragments, and their provenance |
| `domios persona reload` | `persona.reload` | Re-read `<data-dir>/role-presets/*.toml` without a restart or rebuild |

Overlay files add leaf kinds only; one that declares capability, claims an anchoring or Team-authority base, overrides a builtin kind, or owns a contract without the coordination core is refused with a reason and skipped.

## Account

Domios account commands and routes: `domios docs ACCOUNT`.

Runtime provider sign-in is available from the Chat banner or CLI:

```bash
domios agent login --tab <ID>
domios agent login --tab <ID> --device
domios agent login --tab <ID> --show
domios agent login --tab <ID> --show --mesh <PEER> --json
domios agent login --tab <ID> --api-key-stdin
domios agent login --tab <ID> --cancel
```

The API key is read only from stdin; never put it in command arguments. Browser
sign-in uses the default system browser. Device sign-in prints the link and code.
Mesh sign-in shows the URL and code for the user to open; API keys stay on the
provider's own machine. Completion verifies the account and resumes the tab's
same thread without restarting or replaying input. Only one sign-in per provider
can be pending; Cancel releases it before a retry. Read an existing login with
`--show`; this never starts another flow. The response contains `login.url`,
`login.userCode` and `login.expiresAtMs` (Unix milliseconds). Links and codes stop
being exposed at completion, cancellation or expiry. Domios limits disclosure to
15 minutes; a provider-reported earlier deadline wins. Cancel an expired flow
before starting again. The sign-in card's **Send to my phone** requests one
`action_needed` notification through configured phone channels. Login values
travel only in the push payload and remain absent from saved notifications and
sessions.

<!-- revision: 63 -->
