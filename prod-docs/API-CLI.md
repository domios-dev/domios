# CLI and environment API

CLI identity rules, environment variables, agent integration, and desktop behavior.

## CLI

Use `codeterm <command> --help` for the installed CLI contract.

Message and pad file options accept `-` as UTF-8 stdin, including
`plan task dispatch --message-file -`, `send --from-file -`, and `--from-pad -`.
For `plan create`, an implicit project path from a Team-bound caller is the
caller Team workspace cwd; explicit `--project` remains an override.

### CLI JSON ID Contract

Planner mutation commands emit one JSON envelope:
`{ok, kind, id, data, hints?, error?}`. `id` is the primary created or mutated
entity id, and `data` contains the raw daemon payload with typed fields such as
`plan_id`, `task_id`, `spec_id`, and `section_id`. For example,
`spec create --json` returns `kind == "spec"` and `id == spec_id`,
`plan create --json` returns `kind == "plan"` and `id == plan_id`, and
`plan task add --json` returns `kind == "task"` and `id == task_id`.

`plan task dispatch <plan> <task> --mesh <peer-instance-id> --execution-intent mutating`
starts origin-owned remote work. The Worker ACKs and reports to that attempt;
after the Orchestrator accepts the report, run
`plan task merge-remote-lane <plan> <task> <attempt>` on the origin to fetch the
authenticated peer lane at the accepted report SHA and create a real merge
commit into the clean dispatch-time branch.

### CLI JSON Errors

With `--json` every command prints exactly one JSON object on stdout for success
and for failure, including argument errors; stderr stays empty. A failure is
`{"ok":false,"kind":"error","id":null,"data":...,"hints":[],"error":{"code","message","hint","rpc_code"}}`
and the command exits non-zero. `error.code` is one of `not_found`,
`invalid_argument`, `conflict`, `rate_limited`, `refused`, `unavailable`,
`internal`, or a command-specific snake_case code; a missing tab, plan, cron job,
peer, or plugin is always `not_found`. `rpc_code` is the raw daemon code or null.
Exit codes: `0` success, `1` failure, `2` invalid arguments, and the
command-specific codes named in `SECRETS` and `MESH-BRIDGE`. Structured daemon error data stays under `data`, including
`invalid_model` fields such as the provider, requested id, closest catalogue
ids, and refresh command. An ambiguous bare id uses `ambiguous_model` with the
provider choices and a message asking for `--agent`. Mesh relay failures keep
the peer's own delivery `code` inside `error`.

### Sandboxed CLI transport

The CLI uses the local named pipe or Unix socket first. If that transport cannot
be discovered or connected, socket RPC and REST-backed CLI commands can use the
authenticated `POST /api/cli/rpc` loopback endpoint. The fallback needs
`CODETERM_API_TOKEN` and either `CODETERM_API_PORT` or `CODETERM_API_URL`; it
always targets `http://127.0.0.1:<port>`, disables proxies and redirects, and
refuses any configured URL that is not exactly an HTTP IPv4 loopback origin.
The endpoint dispatches the same RPC methods as the local socket. This includes
`api.request`, used for REST-backed commands such as plan reads, tab inspection,
notifications, and agent-pad access. The token must be the local API
token and the TCP peer must be loopback; mesh peer credentials are refused.
HTTP success wraps the daemon value in `{result: ...}`, so result objects may
contain their own `error` field without being mistaken for transport failures.

### Mesh CLI failures

`codeterm mesh discover`, `codeterm mesh broadcast`, and
`codeterm mesh stop-broadcast` check the HTTP result before printing success.
When the listener is not remotely ready, the daemon returns HTTP 409 and the
CLI exits non-zero with that refusal. Run these commands again after the mesh
listener is ready.

`codeterm tab list --mesh <peer>` exits non-zero when the peer has no online
mesh route; `(no tabs)` is reserved for a successful response from an online
peer. `codeterm tab get-text --mesh <peer>` requires a PTY-backed remote tab.
If the tab has no PTY, the CLI reports that condition and suggests choosing a
PTY-backed tab.

Use the CLI or documented REST routes; desktop-only controls are available through the desktop UI.

## Environment Variables

Set automatically in every PTY spawned by CodeTerm:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Running inside CodeTerm |
| `CODETERM_VERSION` | `0.4.0` | App version |
| `CODETERM_SOCKET_PATH` | `\\.\pipe\codeterm-1234` | Socket path |
| `CODETERM_TAB_ID` | `a3f1b2c7` | Current tab ID (8-char hex) |
| `CODETERM_WORKSPACE_ID` | `b4e2c8d1` | Current workspace ID (8-char hex) |
| `CODETERM_ROLE` | `Manager` | Structural agent role: `Manager` or `Worker` (empty if no role). The finer kind (`orchestrator`, `planner`, `watcher`, …) lives in `role_profile`, not here; display surfaces render that kind's label. Set at PTY spawn. |
| `CODETERM_PARENT_TAB_ID` | `b4e2c8d1` | Parent Tab ID in hierarchy (empty for a Team anchor or no role). Set at PTY spawn. |
| `CODETERM_API_PORT` | `7685` | API port for the running instance. Set by CodeTerm so CLI and hooks use the correct port (dev: 7686, prod: 7685). |
| `CODETERM_API_URL` | `http://127.0.0.1:7685` | Optional API origin for local CLI fallback; fallback accepts only the exact IPv4 loopback host. |
| `CODETERM_API_TOKEN` | `your-secret-token` | Bearer token for the API. Injected into every PTY when `[api].token` is set, so CLI subcommands and the auth-enforced hook callbacks (`/api/tabs/*/hook/*`, `/api/mem/flush`) can authenticate without per-call wiring. |
| `TERM` | `xterm-256color` | Terminal type |
| `COLORTERM` | `truecolor` | Color support |
| `TERM_PROGRAM` | `CodeTerm` | Terminal name |

## Agent Integration

When a readiness report is refused locally, `codeterm agent report` prints
`REPORT NOT SENT` and sends a diagnostic to the parent. In JSON output,
`refused_report_diagnostic_sent`, `diagnostic_target_tab_id`, and
`diagnostic_error` describe that diagnostic. The former
`diagnostic_report_sent` and `diagnostic_report_id` keys and `report_outcome`
field are not emitted. A refused report remains absent from task history and
does not change task status.

CodeTerm automatically detects Claude Code and other agents:
- **Permission requests** → in-app toast + Windows notification + webhook
- **Task completion** → in-app toast + Windows notification + webhook
- **Agent icon** on tab (orange ✻ for Claude Code)
- **Pinned tab names** — manual rename blocks OSC auto-rename

Skills for Claude Code are in `skills/` directory. Install via Settings → Agents & Skills.

Choose a model with `codeterm agent models <provider>`, which first refreshes a
catalogue that requires live discovery and, when no new snapshot publishes,
warns and lists the registered one. Pass the id with `codeterm agent spawn
[provider] --model <id>`. Without a provider, a uniquely advertised bare id
selects its provider; otherwise spawn uses the configured default provider.
Authoritative catalogues refuse unknown ids before creating a tab and list the
valid ids in the error. Free-form providers validate ids against a published
live model list and pass the id through unchanged when none exists.
Provider-qualified ids select their provider; an unqualified id advertised by
one provider routes to it. If several advertise it, `plan task dispatch`
requires `--agent` and `agent spawn` requires the positional provider.
Ambiguous ids and aliases advertised by another provider are refused instead of
falling through or crossing providers.

Task attempt inspection keeps `requested_model`, `provider_resolved_model`, and
`session_model` separate. The resolved field stays null until launch resolution
identifies the model; the session field stays null until detection reads that
attempt's bound session. A different detected model is stored in
`model_identity_mismatch` and raises one dispatcher notice.

### Open agent shells (`chatBackend`)

Beyond PTY agents, a tab can host an **open agent shell** — a non-PTY chat tab
driven by a `chatBackend` plugin that talks to a local LLM, runs its own ReAct
tool loop inside the plugin sandbox, and renders its active system prompt as an
observable first-message card. Shells are discovered and spawned through the same
surfaces as agents:

- **Discover:** `GET /api/agents/providers` (CLI `codeterm agent providers`) lists
  installed shells alongside PTY agents, tagged `kind: "shell"`. There is no
  hardcoded shell list — core enumerates the `chatBackend` registry. Every row
  carries `availability` (`available` / `unavailable` / `unknown`); the CLI marks
  the latter two inline, and the UI provider picker reads the same field.
- **Spawn:** `codeterm agent spawn <shell-id> [--system TEXT] [--preset ID] [--model M]`
  (or `POST /api/agents/spawn`) routes by capability to the shell. `--system`
  overrides the preset's system prompt for that session, `--preset` picks a preset,
  no flag → the plugin's `defaultPreset`. These and the watcher flags are refused
  for a PTY agent, which has no use for them.
- **Watcher spawn:** `codeterm agent spawn <shell-id> --role Watcher --parent-tab <orchestrator-tab> --charter <TEXT|charter:id> [--interval 30s] [--nudge-cooldown 10m] [--max-actions 3]`.
  `--charter-file PATH` may be used instead of `--charter`. The REST body fields
  are `charter`, `tick_interval_secs`, `nudge_cooldown_secs`, and
  `max_actions_per_tick`. The host opens the shell with `mode: "watcher"` and
  `engine: { kind: "machine", charter, tickIntervalSecs }`, rejects direct sends
  into the watcher tab, schedules `watcherTick`, parses `watcher_verdict`
  messages, and executes only host-allowlisted verdict actions.
- **Open directly:** `POST /api/tabs {chat_backend, system_prompt?, preset?, model?}`.
- **Observe:** the shell emits a `system_prompt` marker message rendered as a
  collapsible "System prompt" card; tool-call / tool-result messages render via the
  existing chat-UI message rendering.



## Command Palette

Command-palette shortcuts and agent-control workflows: `codeterm docs AGENT-CONTROL`.

## System Tray

CodeTerm hides to the system tray on window close instead of quitting. To fully quit, use the tray menu. This ensures cron jobs and background agents continue running when the window is closed.

## Dev/Prod Isolation

Dev builds use identifier `com.codeterm.dev` and are fully isolated from production:
- **API port:** 7686 (prod: 7685)
- **Data directory:** `~/.codeterm-dev/` (prod: `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (prod: `codeterm`)
- **Single-instance:** Disabled in dev builds, so dev and prod can run side by side.

## Role presets

A launch's role text comes from its role preset; task personality, `role_guidance`, and `executor_role` enter as that preset's task overlay.

### Role preset inspection

| Command | RPC | Purpose |
|---|---|---|
| `codeterm persona list` | `persona.list` | Every reachable role kind with capability columns, plus refused overlays and why |
| `codeterm persona show --role <Manager\|Worker> [--kind <id>] [--guidance <text>] [--occasion <id>]` | `persona.show` | The exact text each occasion emits, its fragments, and their provenance |
| `codeterm persona reload` | `persona.reload` | Re-read `<data-dir>/role-presets/*.toml` without a restart or rebuild |

Overlay files add leaf kinds only. Declaring capability, claiming an anchoring or
Team-authority base, overriding a builtin kind, or asking to own a contract
without the coordination core is refused with a named reason and skipped.

<!-- revision: 22 -->
