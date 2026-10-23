# Agent and orchestration API
REST endpoints for agent restart, model switch, spawn, reports, keep-awake, and batches.

#### Agent Restart (POST /api/tabs/{id}/agent/restart)

Restart a live agent through its provider exit command, bounded shell-ready wait and PTY replacement. Resume its recorded session if present; otherwise launch fresh. Unobserved liveness allows fresh launch only with `LaunchBindingUnconfirmed`, no owning launch, a live shell prompt and no verified lineage/journal recovery session; it replaces the PTY and clears the unverified binding without sending exit. Other unobserved states refuse. An observed exited provider recovers in its surviving shell without teardown: select the newest lineage session (journal fallback), require nonce-verification evidence and exactly one owning provider, then resume through normal launch.

Chat tabs with `agentRuntime` resume the tab's recorded thread with a new epoch and nonce, retaining identity and launch options without replay. After `tab rebind`, restart stops the stale runtime before loading the newly recorded thread and its own history. Explicit `fresh: true` opens a new thread under fresh guards. Readiness requires `thread_loaded`; PTY ids are null. Failures degrade and preserve the recorded thread for another `domios agent restart --tab ID`. Handoff refuses with `runtime_provider_handoff_unsupported`.

```json
{"force": false, "grace_secs": 10, "provider": null, "handoff_history": false}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `force` | `bool` | `false` | Skip the graceful exit event wait and tear down the old PTY immediately. Use only when the agent is hung or unresponsive. |
| `grace_secs` | `u64` | `10` | Failure deadline around the shell-ready event wait before the old PTY is torn down. Ignored when `force=true`. |
| `provider` | `string?` | `null` | Relaunch a live PTY agent with this installed PTY provider. A different provider starts fresh and does not resume the source session. |
| `handoff_history` | `bool` | `false` | With a different `provider`, include a bounded, redacted transcript in the target provider's launch task. The source tab's own transcript must be readable. |

**Ready response** (`202 Accepted`):
```json
{
  "restart_accepted": true, "old_pty_torn_down": true, "agent_ready": true, "recovered_in_place": false,
  "old_pty_id": "f0c92c76-1cce-4a65-b5e4-a68c69ae1e2e", "new_pty_id": "b264031d-5a5a-461b-bba6-189405223f80",
  "tab_id": "a3f1b2c7", "provider": "claude", "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "session_was_stale": false, "session_action": "resumed", "full_access": true, "force": false
}
```

`session_action` is `resumed` or `fresh_launch`; CLI success requires `agent_ready: true`. Transition fields are observations: `restart_accepted` records validation/rate-limit acceptance, `old_pty_torn_down` records prior PTY/tree exit, and, for PTY replacement, `agent_ready` requires new `AgentReady` evidence for a distinct `new_pty_id`. Old readiness never qualifies. In-place recovery keeps matching PTY ids and retires the old shell stage first. `session_was_stale: true` means missing JSONL and fresh launch.

**Errors:**
- `404` — tab id not found
- `400` — liveness is unobserved and the tab is not a confirmed bare shell eligible for fresh launch, the provider is unsupported, a chat-backend session is closed, or exit recovery is refused: `exit_recovery_no_live_shell`, `exit_recovery_no_verified_session` (no agent and no nonce-verified prior session), or `exit_recovery_session_not_owned` (no single provider owns that session record)
- `429` — rate-limited (returns `Retry-After` header + `{"retry_after_secs": N}` body); restart is throttled per-tab to avoid restart loops
- `500` — teardown or relaunch failed
- `504` — the replacement PTY did not reach `AgentReady` inside the bounded readiness deadline

Failures include `error` and observed transition fields; `new_pty_id` requires a linked PTY. Rejections precede acceptance. A failed restart never returns ready `202`.

CLI: `domios agent restart --tab TAB_ID [--force] [--grace-secs N] [--provider PROVIDER [--handoff-history]]`.
Provider changes discard source launch settings; history handoff requires explicit opt-in.
With `--mesh PEER`, `--tab TAB_ID` is required and identifies a Tab on that peer; the local caller's `CODETERM_TAB_ID` is never used.

#### Agent Model Switch (POST /api/tabs/{id}/agent/model)

Switch one agent tab to another model. Domios never types a provider model command, because provider pickers can change the machine-global default. The provider descriptor's `model_switch` declares support and timing. For PTY agents, `live` uses the structured-prompt server registered for that exact tab and confirms from that same tab server; when that route is absent or cannot be proven, Domios falls back to `restart_resume`, returns `mode_used: "restart_resume"` and `fallback_reason`, and resumes the tab's own nonce-verified session with the new model launch args and the same effort, arguments and system prompt a daemon restore would use. For `agentRuntime` Chat providers, live controls target the exact bound runtime thread and confirm from its own session record. Busy model and effort changes are staged with `staged: true` and pending confirmation without contacting the provider, then applied when the tab becomes idle within the staging deadline. The tab's restorable launch record follows the new model. RestartResume requires an idle tab. `unsupported` (the default) refuses the switch.

```json
{"model": "sonnet", "reasoning_effort": "high"}
```

An `agentRuntime` Chat provider may apply settings live to the exact bound thread, or declare `next_turn` and apply model/effort on its next turn. Codex structured-control sessions use the live path: a confirmed settings control returns `mode_used: "live"`, `model_confirmation: "observed"`, and `evidence: "live_session_record"`. Runtime modes come from the provider's `structured_control.runtimeModes` capability and require the bound runtime. Settings stay per-thread. Runtime failure is visible and retryable through exact-thread resume; it never selects a terminal fallback. Providers declaring RestartResume keep their idle-only switching contract.

`reasoning_effort` is checked against the selected model's published effort levels, falling back to the provider's list when the model publishes none. `runtime_mode` is checked against the declared structured-control modes. Both checks happen before runtime control, tab identity updates, or persistence; unsupported values return `400 unsupported_reasoning_effort` or `400 unsupported_runtime_mode` with a `supported` array. On restore, a saved value no longer present in the catalogue is cleared before runtime settings are built; the model's default is used and a warning is logged.

The live session GET or, after a RestartResume fallback, the current binding's launch report or first post-relaunch assistant turn can confirm the model. A live request whose session record does not show the requested model returns pending with `confirmation_detail`; no screen output or POST status is treated as confirmation. After the bounded RestartResume confirmation deadline, a ready relaunch with no model observation still succeeds as pending; the label and `agent_session_model` follow the first turn when it arrives. A live request failure or a confirmed different RestartResume model remains an error.

Confirmed success (`200 OK`) includes `mode_used`, `model_confirmation: "observed"`, `observed_model`, and `evidence` (`live_session_record`, `session_start_report`, or `first_post_relaunch_turn`). RestartResume fallback includes `fallback_reason`. Live success has no `restart` object.

Pending success includes `ok`, `tab_id`, `provider`, `mode_used`, `requested_model`, `launched_model`, `session_id`, `relaunch_evidence` and `restart`.

`relaunch_evidence.session_id` is the resumed session id, `launched_model` is the model passed through the provider's `--model` launch argument, and `agent_ready` confirms the relaunch reached readiness. Pending success omits `observed_model` and `evidence`; the tab identity updates when the first post-relaunch turn provides a model.

Every error body is `{"ok": false, "error": "...", "reason": "..."}` plus the fields below:
- `404 not_found` — tab id not found
- `400 invalid_model` — the model is not in the provider's catalogue; `model_error` carries `valid_model_ids` and `refresh_command`
- `400 unsupported_reasoning_effort` / `unsupported_runtime_mode` — requested control is absent from the selected model/provider catalogue; `requested` and `supported` identify the refused value and choices
- `409 model_switch_unsupported` — the provider declares no live switch; `provider` names it
- `409 not_switchable` — no agent, the agent is not observed live, it is mid-turn or waiting on a prompt, the tab has no verified resumable session, or the tab is the General Agent (its model comes from Settings)
- `429 rate_limited` — the tab restarted moments ago; `retry_after_secs` and `Retry-After`
- `500`/`504 relaunch_failed` — teardown, relaunch, readiness or rebinding failed; `restart` carries the observed transition fields
- `502 model_mismatch` — the current binding's launch report or first post-relaunch turn names a different model; `launched_model`, `observed_model`, and `restart` are included. Missing model confirmation by its deadline is a pending success, not an error.
- `502 live_switch_failed` — the tab server advertised the live route, but the POST failed. `mode_used` is `live`; Domios does not silently restart after a live request error.

CLI: `domios agent model --tab TAB_ID [MODEL] [--force] [--reasoning-effort LEVEL] [--runtime-mode MODE] [--speed TIER|default] [--json]`; omit `MODEL` when changing only a setting. The CLI validates effort and mode against the selected provider catalogue before sending the request. Service tiers can be listed with `--list-speeds`; `GET /api/tabs/{id}/agent/service-tiers` reads that exact tab's runtime catalogue, returning available model-scoped tiers and bound requested/confirmed selections. Typed refusals include `no_runtime`, `no_tiers`, `unsupported_reasoning_effort`, `unsupported_runtime_mode`, and `not_found`. `--json` prints the success body, including `mode_used`, `model_confirmation`, and `evidence` when observed, or a non-zero error envelope carrying the refusal and supported list as `data`.

### Plan task attempts and Teams

Attempts, respawn, dispatch preflight and Team lifecycle: `domios docs API-TEAMS`.

### Keep-Awake (GET/POST /api/keepawake*)

Global sleep/display inhibition; all endpoints require Bearer authentication.

**Status shape (`KeepAwakeStatus`):**
```json
{"active": true, "display": false, "expires_at": 1744387200000}
```
- `active` — whether a keep-awake session is running.
- `display` — whether the display is also kept on.
- `expires_at` — unix milliseconds when the session auto-expires, or `null` for an indefinite session.

**GET /api/keepawake** returns `KeepAwakeStatus`; inactive is `false`, `false`, `null`.

**POST /api/keepawake/start** — optional body/fields:
```json
{"duration_secs": 3600, "display": false}
```
- `duration_secs` — session length in seconds. Omit or `null` for an indefinite session.
- `display` — `true` to also prevent display sleep/dimming. Default `false`.

Replaces any existing session; returns `KeepAwakeStatus`.

**POST /api/keepawake/stop** — no body. Stops any active session. Returns `{"ok": true}`.

**Tauri event `keepawake-changed`** carries `KeepAwakeStatus` on start, stop, expiry and macOS `caffeinate` spawn failure.

**Persistence:** state is written to `~/.domios/keepawake.json` (dev: `~/.domios-dev/keepawake.json`) and restored on startup: indefinite sessions resume as-is, timed ones with the remaining duration, expired ones are discarded.

**Platform support:**
- **macOS** — spawns `caffeinate -i -s` (plus `-d` when `display=true`).
- **Windows** — calls `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, OR-ing in `ES_DISPLAY_REQUIRED` when `display=true`.
- **Linux** — no-op; logs a warning and still returns a status object.

### Agent Spawn
Full request, placement, model, and launch contracts: `domios docs API-SPAWN`.

### Remote mesh workers

A worker spawned on another peer with `domios agent spawn <agent> --mesh PEER --cwd <peer-workspace> [--report-to <tab|main|general>]` gets a **durable cross-mesh report target**: the CLI relays the spawn to `PEER`, whose relay ingress stamps an authenticated `origin_peer` onto the body, and the spawned Tab persists `report_target_mesh_peer = <caller's peer>` + `report_target_tab_id = <report_to>` (defaults to the caller's Tab). `--cwd` is required; `--task-file` is read locally and only its body is relayed.

The worker runs `domios agent report "..."` (no `--mesh`/`--tab`); its daemon relays it over the signed `/send` path (`kind: "report"`), so it surfaces on the origin peer as `[Report from <worker>@<peer>]`, like a local report. If no target resolves, `agent report` fails with an actionable hint, never a silent drop.

Report routing precedence: **remote mesh target → `parent_tab_id` → `report_target_tab_id` → `spawned_by_tab_id`**; never self.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/mesh/worker-transfer-info` | `{worker_tab_id, caller_tab_id?, owner_confirmed}` | Signed Full-access lookup of the current remote Worker session and parent, authorized by that parent or owner. |
| POST | `/api/mesh/worker-transfer` | `{operation_id, worker_tab_id, worker_session_id, expected_parent_tab_id, destination_tab_id, destination_team_id?, caller_tab_id?, owner_confirmed?}` | Signed, Full-access origin-peer operation to change an unbound remote worker's report target in place. |
| POST | `/api/mesh/worker-finalize` | `{worker_tab_id, worker_session_id, expected_parent_tab_id, force}` | Signed Full-access finalization of an exact remote member; stale bindings release membership without killing a replacement. |

The signed origin attests the caller or explicit owner authorization. The worker daemon checks that origin against the worker's persisted report peer and authenticated ownership, the exact worker session and current parent. Only that parent or the owner may transfer. Local children, local Team members, Managers, foreign-owned tabs and task-bound Workers refuse. Success returns `{operation_id, worker_tab_id, worker_session_id, origin_peer_id, previous_parent_tab_id, destination_tab_id}` and keeps the running session and launch identity. The origin validates the caller's exact session and nonce and the destination's live local Orchestrator authority, then durably prepares intent before the call. The worker records its receipt before changing routing; matching retries return that receipt and never repeat a superseded transfer. A lost response or failed local membership commit retains recoverable intent. Remote membership is durable by peer, tab and exact worker session, independent of report telemetry expiry.

### Plan-authorized task-less evidence reports
The authenticated live Plan owner grants review/canonical-QA reporting via `plan.review.authorize-reporter` / `domios plan review authorize-reporter`. The grant stores exact tab/session ids and a one-way nonce digest; Plan history never exposes the bearer nonce. Reports carry only `plan_id`, no task/status/provenance. Success appends immutable `plan_evidence_report`, emits `agent-report` to the live owner and returns `plan_report` with stride id and `task_transition:false`. Capability grants, not role labels, authorize. Missing caller session resolves only from the exact registered tab with matching stored/hook nonce; explicitly wrong sessions refuse. Foreign, unbound, stale/rebound, task-bound or ungranted callers mutate neither history nor report store. Session changes require a fresh grant. Planner/task-bound Worker paths are unchanged.

Each inbound mesh report registers the worker in the orchestrator-side registry (`GET /api/remote-workers`, `remote-workers-changed` event); the sidebar renders it as a read-only "remote tab" ghost node under its orchestrator (peer chip + status, no local PTY).

### Batch Commands (POST /api/batch)

Run multiple operations sequentially in one HTTP request. Later commands reference a command's `id` as `$id`.

**Request body:**
```json
{
  "commands": [
    {"method": "spawn", "id": "boss", "params": {"title": "Boss", "description": "Coordinates repository work", "role": "Orchestrator", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
    {"method": "spawn", "params": {"title": "Worker-1", "description": "Analyzes the code", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude", "task": "analyze code"}},
    {"method": "spawn", "params": {"title": "Worker-2", "description": "Runs focused tests", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude"}},
    {"method": "notify", "params": {"title": "All spawned", "body": "4 agents ready"}}
  ]
}
```

**Available methods:** `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`

**$ref syntax:** any string value starting with `$` is replaced with the `tab_id` from the result of the command with that `id`, e.g. `"parent_tab_id": "$boss"` resolves to the "boss" spawn's tab_id.

**Response:**
```json
{"results": [{"tab_id": "...", "tab_id": "...", ...}, {"ok": true}, ...]}
```

Failures return `{"error":"message","method":"..."}`; later commands still execute.

**Batch method parameters:**

| Method | Required params | Optional params |
|--------|----------------|-----------------|
| `spawn` | — | `agent`, `agent_name`, `title`, `description`, `cwd`, `role`, `parent_tab_id`, `can_spawn`, `task`, `args` |
| `send` | `tab_id`, | `text`, `enter` (default: true) |
| `rename` | `tab_id`, `title` | — |
| `close` | `tab_id` | — |
| `role` | `tab_id`, `role` | `parent_tab_id`, `can_spawn` |
| `notify` | — | `title`, `body`, `level` |
| `can-spawn` | `tab_id` | `can_spawn` (default: true) |
| `report` | `tab_id`, `message` | — |

Plan surface contracts — planner authority over task structure, partial `plan goal set`, lane persistence on dispatch, and recording an external executor: see `domios docs API-PLAN`.

A provider declaring model-provider configuration support accepts `preset` on PTY spawn: Domios resolves its owned model and agent launch arguments before tab creation. See `domios docs INTEGRATIONS` for endpoint keys and preset commands. Provider changes publish versioned catalogue events; selectors refresh without reloading. OpenCode advertises per-model variants as reasoning levels, passed at launch and live switch. Model-switch responses and the chip expose the live or restart/resume path.

### Background work and session forks

With `background_work_guard` enabled, restarts and restart-based model switches return `409 background_work_running` with `running_items`. Provider updates defer until roster drain. Owner `--force` overrides with a visible warning.

`domios agent fork --tab TAB_ID` / `POST /api/tabs/{id}/agent/fork` requires declared fork support and opens a new local tab, leaving the source intact. Response: `tab_id`, `source_tab_id`, `source_session_id`, `session_id`, `binding_pending`. Pending is not binding proof: inspect until launch evidence binds a distinct session. The tab menu uses the same capability; mesh is unsupported.

### Model-free capabilities

`spawn.capabilityProbe: stream_json_initialize` supports `capability_probe_enabled` (default false). `POST /api/tabs/{id}/agent/capabilities/refresh` refreshes the project/binary cache: `status` is `pending`, `ready` with typed `data`, or `unavailable` with typed `reason`. Binary/login changes invalidate and rebuild. Render/switch/send never probe. Cached commands carry argument hints; the composer badge shows subscription/modes. No user turn or usage request is sent.

<!-- revision: 72 -->
