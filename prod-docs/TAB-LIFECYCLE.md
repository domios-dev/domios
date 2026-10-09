# Tab lifecycle journal and unconditional restore

Domios keeps one append-only journal of every authoritative tab lifecycle transition. The journal is **telemetry**: a forensic record of what each launch committed and when. Normal session restore does not use it as authority.

**When Storage is available, tab restore is unconditional.** Whatever tabs were open at shutdown come back. A claimed PTY session re-establishes its provider only from its own provider session record; without unique ownership the restored tab remains a shell. Journal state — missing, mismatched, truncated, or absent — does not change that decision.

## Storage authority and recovery

The instance's desk is owned by one Storage authority in `storage/state.db` beneath its data directory. Storage opens before restore; an open, import, or archive-verification failure leaves desk state unavailable and prevents a default-empty desk from replacing it. The typed importer preserves recognized source files under `storage/legacy-originals/`.

For backup, stop Domios cleanly and copy the complete `storage/` directory, including `state.db`, referenced assets, and the import archive. Restore into an empty `storage/` root with a Storage-compatible build. An older binary cannot see changes made after cutover in retained legacy files, so running it against those files is not rollback. Keep the backup and originals intact and use a compatible recovery path.

## The restore contract

1. Storage loads the committed desk and every tab is rebuilt with its persisted `agent_session_id`, binding nonces, `role`, `role_profile`, `team_id`, `parent_tab_id`, `can_spawn`, `depth`, model provenance, and chatBackend binding intact. Prior binding lineage is restored for every restored tab, whether or not it claims a session.
2. A PTY agent tab resolves the unique provider whose store owns that tab's claimed session id, then relaunches that provider with the session (`--resume`-style continuation). A missing provider label does not bypass that lookup: a unique owner repairs the label before either visible or lazy restoration claims the launch. The repair preserves the session and checks the tab's identity again before committing; it never replays a historical task. Missing or ambiguous ownership retains the saved session and refuses agent materialization. A provider proves ownership from its authoritative per-session record, which exists from the session's first turn, never only from a secondary index the provider writes later: a session that has not reached that index yet still resumes. On an OpenCode resume only the explicit `--session` id is validated; no session is inferred from the latest record for a cwd.
3. If a claimed session has no unique provider record, the tab comes back as a shell with a null provider label. A snapshot with no claimed session may still fresh-launch its explicitly persisted provider. Restore never chooses a claimed session's provider from another tab, the newest session for a cwd, or the journal.
4. Pre-journal snapshots restore like everything else. Lineage fields are optional telemetry, not an admission ticket.
5. A rebuilt record is not a restored agent. The restore report counts a pane as restored only once an agent process is observed live behind it. The persisted report is reconciled whenever lifecycle evidence changes, so a late-live pane replaces its earlier dropped outcome; each pane gets one log line per restore wave. A pane that came back as a bare shell is named with its role and reason (`agent_not_live`), and one whose liveness is not yet observable is reported as `agent_liveness_unobserved` rather than assumed either way.
6. A provider exit observed while a graceful shutdown is armed (update, quit, restart, signal) keeps the tab's provider, session and nonces. The committed desk state still names the session the next boot resumes. A failed launch preserves the provider, role/profile, saved session and exact restore data and keeps its failure card. Only an exit after the launch authenticated its session clears identity into binding lineage. Recovery accepts this tab's own nonce-verified session or journaled launch target, requires its provider to confirm the exact session exists for the tab's cwd, and binds it under a new nonce. `agent restart` resumes that session in place with the launch arguments its spawn recorded (model, effort, arguments, identity instructions). When a tab instead has `LaunchBindingUnconfirmed`, a live shell-ready PTY, a known provider and no saved session or recovery target selectable from its lineage and lifecycle journal, `agent restart` starts that provider fresh from its recorded launch and reports `fresh_launch`. A provider started by hand in that surviving shell inherits the exited launch's nonce; when its session-start hook reports that newest retired nonce and no launch owns the tab, the tab adopts it as a verified binding and reaches `agent_ready`.
7. A team member whose provider exited to its shell before shutdown restores as that shell and is not relaunched. The report names it `agent_exited_before_shutdown` with `exited_session_id`; `agent restart` recovers it.
8. A restored agent whose saved cwd is no longer a directory is not relaunched anywhere else: the Tab keeps its session and cwd, shows `spawn_failed` naming the missing directory, and raises a notification. Restore the directory, then `codeterm agent restart --tab ID`.
9. Restore never replaces an unavailable exact saved session. When its provider confirms the session is missing, the failure card offers **Start new session** beside **Keep shell**. The explicit action, also available as `codeterm agent restart --tab ID --fresh`, preserves tab identity, cwd, Team, plan binding, model and effort while clearing the dead session and exact-restore binding. An existing exact session is refused; a live agent requires `--force`.

## Resume after restart

An agent tab that was mid-turn when Domios stopped continues its work after the next start. The stop can be a quit, an update, a self-install, a crash or a kill; no shutdown hook is involved.

- **The working mark is persisted continuously.** Each live turn edge of a PTY agent tab (working, or idle/question) updates its `turn_resume` mark in Storage: the tab's own `session_id` and the `start_epoch` that observed it. A turn that ended before the stop clears the mark. Once a graceful shutdown is armed the mark is frozen, so teardown never erases the turn it interrupted.
- **One continue per tab per start.** At start, a mark recorded by an earlier start and not yet settled is owed. The tab receives one system message from Domios through the post-ready send path, only when it is `agent_ready` with a nonce-verified binding on the same session the mark recorded. The claim (`settled: {start_epoch, outcome: "sent"}`) is saved before the payload is written, so a later crash never produces a second continue for the same turn. Work the agent resumes after that continue records a fresh mark for the following start.
- **Skips are recorded, never retried.** A tab that resumes on another session, comes back as a shell, has no PTY agent session, or is not agent-ready within the 300-second failure deadline is settled with `outcome: {"skipped": "<reason>"}` (`session_not_resumed`, `no_agent_session`, `no_pty_agent_session`, `spawn_failed`, `session_unverified`, `agent_not_ready`, `claim_not_persisted`, `session_changed_before_send`, `send_refused`, `disabled`). Idle tabs carry no mark and receive nothing. Owner messages are never replayed.
- **A requested continuation shares the same owner.** `update install --from-file` (and `build.sh install-local`) passes `--continue-tab`/`--continue-prompt` to `/api/session/save`; the tab's mark then carries that prompt, survives the caller's turn ending, and is delivered once in place of the default text. The installer helper itself never writes to a tab.
- `[agent] continue_after_restart = false` (`codeterm docs CONFIG`) disables delivery; owed marks settle as `disabled`.

The mark is visible in the tab API object as `turn_resume`. The first start of a build that introduced it has no marks from the previous build to resume.

## Restore reporting and observed liveness

`state/recovery/last_restore_report.json` is written when the wave is armed and whenever lifecycle evidence changes. A dropped or unobserved pane remains eligible for reconciliation, so a later live observation corrects an earlier absence in the report. The daemon logs one pane line per tab for the wave. The report also records `previous_epoch_end` — `clean_shutdown:<reason>` or `crash:<classification>` — so a restore that lost every agent to a planned update reads as that, and an unexplained one cannot hide behind a plausible story.

Restored agent PTYs are materialized by the backend once the frontend reports ready, and still when it misses its readiness deadline: the deadline is never a reason to abandon relaunch. A wave materialized without the frontend records `backend_materialization: "frontend_readiness_deadline_elapsed"`, so panes still unobserved name what the restore was waiting on.

Elapsed time or a rebuilt tab record alone never proves a live restored agent: liveness is observed from a live PTY session, a real launch transition, the shell's own finished-command report, or an open plugin session.

A launch that still owes its startup acknowledgement is not ready while its provider shows the owner a prompt: an `agent_ready` tab moves to `awaiting_owner_prompt` when that prompt surfaces and returns to `agent_ready` when it clears. A user-owned prompt also raises one action-needed notice for its blocker kind during that launch; the notice names the tab and what it needs. If the tab has a parent agent, Domios routes the parsed question and options there with instructions for `codeterm tab respond`. Owner-launched tabs keep the prompt owner-facing and do not receive automatic keys.

A pane observed `absent` is reconciled through the ordinary lifecycle teardown, so it keeps no `agent_state`, elapsed timer, status line, `agent_session_id`, or model label, and the existing guarded-input refusal arms with it.

## Where the journal lives

| Instance | Path |
|----------|------|
| Production | `~/.codeterm/sessions/tab-lifecycle.jsonl` |
| Dev | `~/.codeterm-dev/sessions/tab-lifecycle.jsonl` |
| Named dev instance | `~/.codeterm-dev-<name>/sessions/tab-lifecycle.jsonl` |

The journal remains under the instance's `sessions/` directory. It is telemetry and is not a Storage restore source.

**Agent access is read-only.** Read, grep, or diff it; never hand-edit the journal or a snapshot.

## Format

One JSON object per line, in journal order. Two record kinds:

```jsonc
// a committed transition
{"record":"transition","entry":{
  "seq":41,
  "key":{"tab_id":"db79d2da","epoch":3,"operation_id":"6f1c…"},
  "transition":{"transition":"session_bound","session_id":"…","binding_nonce":"…","source":"hook"},
  "recorded_at":"2026-08-04T11:20:31Z"
}}

// a compacted operation
{"record":"checkpoint","checkpoint":{
  "key":{"tab_id":"db79d2da","epoch":3,"operation_id":"6f1c…"},
  "committed_through":46,
  "authority":{"provider":"claude","session_id":"…","binding_nonce":"…",
               "role":"Manager","role_profile":"orchestrator",
               "team_id":"team_…","can_spawn":true,"depth":1},
  "agent_type_write":{"origin":"spawn","new_provider":"claude",
                       "session_id":"…","nonce_verified":true,"applied":true,
                       "recorded_at":"2026-08-04T11:20:32Z"},
  "sealed_at":"2026-08-04T11:20:33Z"
}}
```

### Lineage

`key` is the whole identity of one lifecycle operation:

- `tab_id` — the tab.
- `epoch` — monotonic per tab; a new one opens at every launch.
- `operation_id` — a UUID minted with the epoch.

A tab's own persisted lineage (`agent_identity.lifecycle_epoch` and `agent_identity.lifecycle_operation_id`) links its snapshot to its journal history for inspection. Restore preserves the lineage fields verbatim and otherwise ignores them.

### Ordering

`seq` is a single monotonic sequence across the whole file. It is the **only** ordering authority. `recorded_at` and `sealed_at` are UTC stamps for reading the journal by eye — they are never consulted for ordering, checkpoint selection, or conflict resolution.

A line that fails to parse, or whose `seq` does not advance, refuses itself and everything after it, and the corruption is reported. The refused tail moves into `tab-lifecycle.refused.jsonl` beside the journal and is never replayed. Restore never reads the journal, but exit recovery proves a session only from replayable `session_bound` or checkpoint records.

### Transitions

| `transition` | Meaning |
|--------------|---------|
| `epoch_opened` | A launch operation begins. |
| `provider_identity_committed` | Provider accepted for this operation. |
| `agent_type_write` | Provider-label write audit: origin, previous/new/attempted provider, caller tab, claimed session, nonce verification, and whether it applied. Parser writes are diagnostic only. |
| `session_bound` | Provider session bound for the tab's current launch nonce. `source` is `hook` (the provider echoed the nonce), `pid_registry` (the provider's per-process record for a process in the tab's own PTY tree, start time matched), or `launch_marker` (the provider's session record carries the marker this launch wrote). The two detected sources run only after the hook missed its launch deadline; records without `source` read as `hook`. |
| `role_binding` | Role/profile/`can_spawn`/`depth` as committed. `role` absent means a role deliberately cleared. |
| `team_binding` | Team and parent as committed. `team_id` absent means unbound. |
| `launch_ready` | **Seals** the operation. |
| `restart_requested` | A restart was accepted; the relaunch opens a new epoch. |
| `restore_awaiting_agent_restart` | Diagnostic: restore did not relaunch a member that exited before shutdown. Never authority. |
| `rolled_back` | Authority withdrawn. The operation can never seal afterwards. |
| `closed` | Tab gone. |
| `permission_auto_confirm` | Diagnostic audit of opt-in permission auto-confirm: `decision` is `enabled`, `disabled`, `confirmed` (with `tool_name`, `option_label`, `rule`, `question`, `prompt_fingerprint`, `session_id`) or `refused` (with `reason`). Never authority; compaction keeps the newest 20 per tab. |
| `remote_ownership_released` | Diagnostic record that local control ownership was released, including the peer and optional caller tab. Never authority; compaction keeps the newest 20 per tab. |

Transitions are recorded when the mutation commits, never inferred afterwards from tab state.

### Checkpoints

A **checkpoint** is a sealed launch that committed its provider and session and was not rolled back or closed. Checkpoints show what a launch actually committed; they carry no restore authority.

## Retention and compaction

The journal is bounded, read-only diagnostic history; corruption is reported, checkpoints do not authorize restore, and compaction preserves surviving evidence.

## Inspecting it

Ordinary filesystem tools are the intended interface.

```bash
JOURNAL=~/.codeterm-dev/sessions/tab-lifecycle.jsonl
# everything that ever happened to one tab, in order
grep '"tab_id":"db79d2da"' "$JOURNAL"
# just the transition names and when they landed
grep '"tab_id":"db79d2da"' "$JOURNAL" \
  | jq -r '.entry | "\(.seq)  \(.recorded_at)  \(.transition.transition)"'
# does this tab have a committed checkpoint at all?
jq -c 'select(.record=="checkpoint" and .checkpoint.key.tab_id=="db79d2da")' "$JOURNAL"
codeterm tab inspect --tab db79d2da --lifecycle
```

Session backup list and restore commands read Storage-backed desk projections. Restore is additive: it recreates missing workspaces and tabs under their original ids without changing or closing existing entries. For one selected live tab, `codeterm tab inspect --tab ID --lifecycle` adds a read-only `lifecycle` object with live lineage, ordered transitions, surviving checkpoint, and journal-tail health. The workspace map from plain `tab list` and the scoped tree from `tab list --workspace` remain separate from lifecycle telemetry. The projection is telemetry only; it has no restore or checkpoint-mutation surface.

## Attempt retirement fences a closed tab's dispatch

Closed attempts cannot become ready or be reactivated; recover through the Plan owner. When the closed tab (or a fenced Worker whose agent process exited) held an `in_progress` task, the plan owner gets one `[worker lost: <task>]` notice and the bus one `worker-lost` event (`plan_id`, `task_id`, `worker_tab_id`, `orchestrator_tab_id`, `worktree_path`, `branch`, `lane` state `dirty`/`clean`/`missing`/`no_lane`/`unknown`, `cause`, `recovery`, `delivery`), keyed once per plan, task, and attempt. When the plan owner's tab has no live agent (a bare shell), the notice is withheld rather than typed into the shell (`delivery` `withheld_no_agent`), the user gets one alert naming the affected tasks, and each withheld notice is delivered once when the owner's agent is ready again. An attributed `tab close` of such a Worker is refused (`busy worker close refused`, `409`) unless `--force`; a human close proceeds and returns `warnings`. `plan doctor` reports remaining `in_progress_tab_gone` drift as an error.

## Not in scope

- **Tab creation** is not journaled; the first `epoch_opened` marks its lifecycle.
- **chatBackend lifecycle authority does not exist.** Those tabs may have detached provider-label audit records, but no lifecycle operation or checkpoint.
- **Rollback to a named checkpoint** is not implemented. The journal records history; it does not replay it.

## General Agent recovery

Recovery observes launch settlement, startup deadlines, PTY death, provider exit to a surviving shell, idle and busy without output. The watchdog uses those events and checks shutdown, launch grace, PTY liveness and restart bounds when each event occurs. A live PTY never authorizes a restart from screen evidence. Confirmed dead-PTY recovery retains the watchdog predicates independently of model availability.

The connected observer ignores confidence below 0.60. Model-only owner notifications require two agreeing observations at or above 0.80; model-authorized automatic actions require both observations at or above 0.90 and deterministic evidence:

| State | Recovery authority |
|---|---|
| Update prompt | Agent-owned prompts use one calibrated choice at 0.80 or above among provider-declared options. Owner-launched tabs keep the prompt for the owner. |
| Crashed to shell | Confirmed provider exit event, provider absent from this tab's process tree and live shell PTY; resume a verified recovery session, otherwise launch fresh. Jev is audit-only. |
| Starting past deadline | One restart, with provider absence or a dead PTY. |
| Error loop | One restart, then notify; provider absence or a dead PTY is required. |
| Login or unavailable model/quota | Owner notification with choices that match the blocker. |
| Folder trust | Existing trust responder checks provenance, shared trust cache or a live same-provider tab with a verified session in the exact canonical directory. |
| Command/tool permission | The tab's own permission policy; the observer does not approve it. |
| Other | No recovery. |

Automatic model recovery remains disabled until a separate human-labelled held-out set reaches 0.95 precision in the 0.90 automatic band and the live recovery oracle is recorded. Each automatic dispatch first writes the Supporter journal and emits a verbose notification. At most two automatic restarts are reserved durably per General Agent episode, even if configured higher. Exhaustion emits one failure notification and stops recovery until an owner restart resets the episode.

UI-created tabs suppress trust, permission and login owner notifications unless parent/spawner, team, cron, hidden-worker or General Agent ownership applies. The journal still records observations. Restored-tab notices are held for restore-wave settlement and grouped by state. Durable tab/state/launch keys prevent repeated notices across application restarts; a new launch can notify again.

## Unattended launch prompts

Agent-, Team-, worker- and cron-owned tabs may encounter prompts while visible or hidden. Tabs launched directly by the owner without agent or Team ownership keep owner-facing prompts.

- For folder-trust, tool-permission and update prompts, one calibrated Supporter choice at 0.80 or above may select any provider-declared option. Domios submits that option through the prompt response surface; it never sends model-authored text.
- If the calibrated choice is unavailable, fails, is capped, selects `other`, or is below 0.80, the existing structural responder decides. A prompt it cannot resolve is routed to the owner with its parsed options; user-audience tabs also raise one owner notice per blocker kind and launch, and include the parent agent when one exists.
- Login, authentication, unavailable-model and quota prompts are always routed to the owner. Folder-trust choices at launch or in cron do not require host provenance for the model decision; provenance continues to govern other trust decisions.
- Each routing decision enters the Supporter journal as `owned_prompt_answer` with `decided_by` set to `adapter` or `fallback(<reason>)`.

Prompt choices are journaled without raw screen text or credentials.

Hidden agent PTYs launch at 120x40, and their screen parser is sized to that PTY geometry, so a launch prompt is recognized from the screen the provider actually drew.

## Provider updates

The production instance automatically coordinates provider updates. This replaces
owner consent offers with an automatic update and one verbose summary. Named and
non-production instances participate only with
`updater.allow_provider_updates_in_non_prod = true`.

A provider update check or a classified ManualUpdate prompt starts the
provider-declared update command in a utility process with a 300-second failure
deadline. Provider and target-version claims are durable and shared across
instances: the command runs once per target version, with one run per provider
at a time. Prompts use only the provider-declared safe option through the prompt
response API; Domios never chooses Update now inside an agent tab.

Working tabs keep running throughout the update. On success, idle tabs,
including the General Agent, restart through `agent restart` with verified
session resume (`resumed`), or the pipeline's eligible `fresh_launch` fallback.
A tab with an active turn or an in-progress task attempt stays pending until its
next idle or task-settlement event. Each tab gets at most one automatic restart
for that update. On command failure, no tab restarts; the owner receives one
failure notice with bounded captured stdout and stderr. An interrupted durable
claim is retained to prevent an unconfirmed updater from running twice.

The summary reports restarted, pending and failed restart counts, and updates
in place as pending tabs finish. Update runs, safe prompt answers and individual
restart outcomes enter the Supporter journal.

<!-- revision: 46 -->
