# Agent spawn API

ACP profile setup, registry discovery and authentication: `domios docs ACP`.

One-call agent creation, launch validation, placement, model routing, and response contracts.

### Agent Spawn (POST /api/agents/spawn)

Creates a new tab in the parent's workspace, assigns its role and launches the provider with an optional task. Each agent owns a tab; hierarchy is logical. Spawn never changes the active tab. Launch verdicts are bounded below.

**Request body:**
```json
{"agent":"opencode","title":"Reviewer","cwd":"C:/project","role":"worker","parent_tab_id":"a3f1b2c7","task":"Review the changes","preset":"demo"}
```

Optional `launch_profile` selects an ACP launch profile declared by the provider (`domios agent spawn acp --profile <id>`). It is separate from `model` and `role_profile`. Providers declaring launch profiles require one. Before tab, Team-member or attempt creation, unknown, missing required and uninstalled profiles return HTTP 422 with `error.code` `unknown_launch_profile`, `launch_profile_required` or `launch_profile_not_installed`, plus `provider`, `requested_profile` (null when missing), and `declared_profile_ids`. The CLI prints the code and exits non-zero. Local task dispatch accepts the same `--profile`; respawn inherits the source attempt's profile unless overridden or the provider changes.

The selected model's advertised reasoning levels constrain `reasoning_effort`; the provider's flat list applies when the model has none (`domios agent models <provider>` shows the sets). Provider listings may include `default_reasoning_effort` on the provider descriptor (fallback) and on each `available_models` entry (that model's live default). `domios agent models <provider>` marks the model-specific default beside its reasoning levels, falling back to the provider value only when the model has none.

`domios agent models <provider>` requests a catalogue refresh before every listing: providers that declare `liveDiscoveryRequired` re-probe and warn when the probe could not refresh, while any other catalogue gets the reader-gated re-probe (its TTL window, or a changed credential file, provider config, or provider binary). The authenticated `POST /api/agents/providers/refresh` endpoint accepts `{provider_id}` for the same explicit refresh; a failed probe keeps the last registered snapshot and reports `refreshed: false` with a `reason`. The CLI warning quotes that reason; with `--json`, stdout stays the model array and stderr carries `{"warning":"live_model_discovery_not_refreshed","provider","reason"}`.

Fields are optional; `agent` defaults to the configured provider unless a unique model selects another. `cwd` must be absolute and exist on the receiving host or spawn refuses before tab creation; the CLI resolves relative `--cwd` locally. Titles: one word, at most 64 characters. `description`: one line, at most 200 characters. Both refuse control characters. Every Domios agent defaults to full machine access for every role and either execution intent, through the provider's declared unattended/bypass mode. Neighbors concierge launches retain their authority-bound confinement. Optional `sandbox: "read-only"|"workspace-write"` explicitly requests provider confinement for this launch; unsupported mappings are refused. When the caller (`X-CodeTerm-Tab-Id`, `spawned_by_tab_id`, `creator_tab_id` or `parent_tab_id`) has a recorded sandbox, or `agent.sandbox_ceiling` is set, a `sandbox` above that ceiling, or none, is refused with `403 sandbox_ceiling`; under a workspace-write caller the `cwd` must lie inside its writable roots. The removed `agent.full_access` setting never gates launches. Saved sandbox policies remain immutable. Without full access, a model the provider declares without an unattended permission mode receives launch-scoped grants for the role's Domios commands, and an Orchestrator or Planner on such a model is rejected before tab creation with an error naming the model (`team pair` checks both members first). `audience` controls user notifications only. With an authoritative model catalogue, an unknown `model` is rejected before tab creation (`domios agent models <provider>` refreshes and lists valid ids); free-form providers validate against a published live model list and pass ids through only when no live list exists. An id advertised by several providers requires an explicit provider, and provider aliases are never passed across providers. Reasoning effort is capability-validated before launch, and a level the provider lists without a launch flag for it is refused; response provenance separates host-owned `planned` plan-task intent from client `requested`, provider `resolved`, and exact-session `actual`; ordinary spawn payloads cannot set `planned`. `task` goes through the provider's declared launch delivery; `agent_name` maps to the provider's named-agent launch option when supported. The CLI flag `domios agent spawn --non-user-facing` sends `audience=automation`. A `Worker` with `execution_intent: "read_only"` needs no task binding: it owns and transitions no task, and `authorized_gates` is its whole command scope, rendered into its brief. For a plain Worker, `execution_intent: "mutating"` without `plan_task`/`task_id` is refused; Worker kinds may do assigned mutating work without a task, and `authorized_gates` is rejected alongside `plan_task` (a task-bound Worker declares gates on `plan.task.dispatch`, where they are enforced). Task-bound mutating spawns require `cwd` to be a branched worktree lane owned by this task; missing checkouts, canonical and foreign lanes refuse before tab creation. Use `domios plan task dispatch` to provision it. A worker type (such as `qa` or `release_manager`, or a name the Team registered) launches with that type's own prose in place of the plain Worker's. `domios plan task dispatch` selects it from `role` or, when omitted, from the task's `executor_role` when that names a builtin worker type.

`audience` is `user` (default) or `automation`; an automation or hidden tab's prompts and exits never notify the user and are reported to its parent or report target instead (`domios docs NOTIFICATIONS`). `description` is validated like a tab description, and `reasoning_effort` is never silently downgraded. Routing is **by capability, not identity**: an `agent` matching a `chatBackend` shell id opens an agent shell (`system_prompt`/`preset`/`model` and watcher `mode`/`engine` reach its `openSession` ctx); otherwise the PTY agent path, which refuses the chatBackend-only fields (`system_prompt`, `charter`, `tick_interval_secs`, `nudge_cooldown_secs`, `max_actions_per_tick`; `preset` is accepted only through a declared model-provider configuration capability) and names them, just as a chatBackend shell refuses PTY launch `args`. `role` names a role type: a manager slot (`orchestrator`, `planner`, `cartographer`, `curator`), the host-only `watcher`, or a worker type available to the Team (`worker`, `qa`, `reviewer`, `release_manager` or a registered name). The structural role derives from the type; `Manager`, `Worker` and the other capitalized literals are refused, and `role_profile` is not read. A `Planner` spawn requires `plan_id` and resolves an omitted parent as the role endpoint does. Watcher spawns require a watcher-capable `chatBackend` shell and `charter`; policy fields are clamped by the host. An unknown id errors with the available agent **and** shell ids. `report_to` sets where the worker's `agent report` lands (Tab id or `main`/`general`). `origin_peer` is **never client-supplied**: the mesh relay stamps it from the authenticated sender so a mesh-spawned worker reports back (*Remote mesh workers*). `team_id` + `creator_tab_id` (`domios agent spawn --team`) attach a ROLELESS agent to an existing Team: `creator_tab_id` must belong to `team_id`, and the pair is rejected with `role` or `plan_task`. Membership is checked before tab creation, so a refusal leaves nothing behind. Socket `team.adopt_agent` applies the same check to an already-running tab; see API-TRANSPORT. A **roled** Team member (including a replacement Orchestrator) uses `POST /api/teams/{team_id}/members` / socket `team.add_member` (API-TEAMS).

Task-bound canonical mutating placement uses parent-authorized `plan task dispatch --execution-intent mutating --canonical-writer`, which reserves one exclusive task attempt before creating the Worker. Direct `agent spawn --plan-task` cannot reserve that attempt and remains refused for task-bound canonical mutation. Task-less Worker kinds with Plan context retain parent-directed placement behavior and do not reserve task attempts or participate in canonical writer occupancy, so the parent coordinates those canonical writes. Local dispatch may pass `--role qa` or `--role release_manager` to select the built-in prompt profile without changing the saved `executor_role`; the kind grants no placement or publication authority.

An unknown id on an authoritative catalogue or a published free-form live list is refused before spawn or task dispatch creates a tab. REST spawn returns HTTP 422 with `error.code` set to `invalid_model`, including `provider`, `requested_model`, `valid_model_ids`, and `refresh_command`. A bare id owned by multiple providers returns `ambiguous_model` with `ambiguous_providers`; choose the provider explicitly. Neither refusal has a transient retry hint. CLI `--json` preserves these fields under the error envelope's `result` for spawn, dispatch, and team pair.

**Response** (after the launch verdict, bounded at 20 s):
```json
{"tab_id": "a3f1b2c7", "pty_id": "c9d0e1f2", "agent": "claude", "title": "Worker-1", "role": "Worker", "launch_state": "ready"}
```

`launch_state` is `ready` once the agent is ready, or `starting` with a `warning` when it waits on an owner prompt, holds a provisional boot stage, or is still launching at the bound; `starting` never claims readiness. A provider that exits before readiness fails the spawn: REST answers 500 with `error.code` `launch_failed`, socket `agent.spawn` returns the same fields as error `data`, and the CLI exits non-zero. Both carry `tab_id`, `reason`, `output_tail` (the provider's last printed lines), and `retry_hint`; the failed Tab stays in `spawn_failed` for inspection. `plan task dispatch` reports the same failure, output tail included, through its startup-failed event.

### Launch profile catalogue and owner setup

| Method | Path | Operation |
|--------|------|-----------|
| GET | `/api/agents/providers/{id}/profiles` | Read the provider's declared launch profiles and installation state. |
| POST | `/api/agents/providers/{id}/profiles` | Confirm an installation or configure an owner-controlled local command. |
| GET | `/api/agents/providers/acp/registry` | Search the ACP registry as the local owner. |
| POST | `/api/agents/providers/acp/registry/install` | Accept an asynchronous pinned ACP registry installation. |
| DELETE | `/api/agents/providers/acp/profiles/{name}` | Remove an owner-added ACP profile and its files unless its setup is active or a tab uses it. |

Both routes require authenticated API access. POST additionally requires the local
owner identity (desktop app or owner's web session); tab, relayed, peer and service
credentials are refused with `403` before setup. A `confirmed: true` body does not
grant owner authority.

GET has no body and returns a JSON array of
`{id, install, auth, canInstall, ownerConfigured, configOptions, setup?}`. `install` is `not_installed`,
`installed`, `detected` or `unknown`; `auth` currently reports `unknown`.
`canInstall` means the declaration has a supported install specification.
`configOptions` is the profile's last declared model and `thought_level` options:
each has `id`, `name`, `category`, `currentValue` and `options: [{value, name}]`.
It is empty before discovery and persists across daemon restarts, bound to the
profile's launch declaration. Cached choices are advisory. To start ACP with a
choice, send `agent: "acp"`, `launch_profile`, `model` and `reasoning_effort` on the existing
spawn request. Omit a choice to preserve the agent's default. After session loading,
the runtime validates against the live declarations and confirms each requested
option before bootstrap submission. Missing, rejected or unconfirmed choices fail
readiness; unsupported values report the offered IDs. Such failures can occur after
the spawn response has created a tab, and appear as runtime readiness errors.
No runtime provider or no declared profiles returns `400` with a text reason;
a catalogue worker failure returns `500` (`Profile catalog unavailable`).

POST requires camelCase `profileId` and `confirmed`; optional `command` contains
only `binary` and `args` (an array of strings). Unknown fields are rejected.
For an installable profile, omit `command`:

```json
{"profileId":"deepseek-harness","confirmed":true}
```

For an `ownerConfigured` profile, supply an installed executable and its arguments:

```json
{"profileId":"local","confirmed":true,"command":{"binary":"/absolute/path/to/agent","args":[]}}
```

There is no environment field. A local command must resolve to an installed file;
shell commands and package-fetching launchers (`npx`, `npm`, `uvx`, `uv`, `env`) are
refused. Installation uses the profile's pinned distribution, verifies its archive
and saves a receipt bound to the profile's binary, arguments and install spec; icons,
limits, launch policy and settings never stale it, and receipts from earlier builds
are rebound on first use without reinstalling. POST returns `202 Accepted` with
`{jobId, providerId, profileId, state:"accepted"}` before installation completes.
The `provider-profile-setup` event carries that job through `installing` to
`installed` or `failed`; a failed job includes an `error` string. GET includes the
latest job as `setup`, so clients can subscribe before reading and recover after
reconnecting without polling or replaying POST. `installed` follows the saved
receipt and readiness invalidation, not a download log. An accepted job continues
if its original HTTP client disconnects. Active jobs are bounded and duplicate
setups for the same profile are refused. Completed job state lasts for this daemon
session; saved receipts remain authoritative across daemon restarts.

Admission failures return `400` with a text reason: missing confirmation, unknown
profile, invalid provider id, unsupported installer, duplicate setup or installer
capacity. Command validation, missing binaries, download/checksum/package failures
and unavailable receipt storage become terminal `failed` jobs. Failure to accept
the setup worker returns `500` (`Profile setup unavailable`). Malformed JSON/schema errors use the standard
Axum JSON rejection. Neither route creates an agent tab.

### ACP registry management

All three ACP management routes require the same local owner identity as POST
profiles. Tabs (including Orchestrators), peers, relayed callers and services receive
`403` before validation or registry access. These REST responses are raw JSON,
without the CLI `result` wrapper.

GET registry accepts optional query parameter `q` (default empty, maximum 256
UTF-8 bytes). Success is `200` with an array of
`{id, name, version, description, iconUrl}`; `iconUrl` is a URL or `null`. Search
uses the CLI's bounded index fetch and 20-second deadline. Invalid queries, fetch
failures and invalid indexes return `400` with the same bounded text cause as the CLI.

POST registry/install accepts only `{"id":"registry-agent-id"}`. The owner action
itself confirms installation; there is no additional `confirmed` field. Success
is `202` with `{jobId, providerId:"acp", profileId:id, state:"accepted"}`. Subscribe
to `provider-profile-setup` before POST. The same job subsequently emits
`installing`, then `installed` or `failed` with optional `error`; GET profiles
includes the job for reconnect/reopen recovery. Before the installed profile is
saved, its job-only row has `install:"not_installed"`, `auth:"unknown"`,
`canInstall:false`, `ownerConfigured:false` and `setup`. It is not yet a configured
launch profile. A failed import retains its job-only status for this daemon session.

Invalid names, existing/built-in profile names, an unavailable ACP provider,
duplicate setup or installer capacity refuse immediately with `400`. Registry
lookup/fetch, unknown registry ids, unsupported distributions, pinning, checksum
and installation failures become terminal failed jobs after acceptance. Installation
uses the same mapping, verification, receipt and profile storage as the CLI;
`installed` is emitted only after the owner profile has been saved. Worker startup
failure returns `500`; malformed or unknown JSON fields use Axum's JSON rejection.

DELETE profiles/{name} returns `200` with `{"id":"name"}` after removing an
owner-added profile from storage and invalidating readiness. Built-ins, missing
owner profiles, invalid names and profiles with active setup return `400` with a
text reason. Installed package files remain on disk. Deletion and CLI mutation
share the same reservation. Terminal setup state is cleared on successful removal.
These routes create no agent tab.

<!-- revision: 15 -->
