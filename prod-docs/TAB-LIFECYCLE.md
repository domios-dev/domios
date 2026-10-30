# Tab lifecycle journal and unconditional restore

With Storage available, every tab open at shutdown returns. The append-only journal is telemetry, never restore authority. Claimed PTY sessions require unique provider ownership or remain shells.

## Utility shell deadlines

Test/QA/build shells need per-command bounds (`60m` recommended; positive `s`, `m` or `h`, maximum 365 days); dev servers take none. Only the creating agent controls deadlines; mesh requests are refused.

```bash
domios tab new --purpose qa --timeout 30s --cmd "sleep 600"
domios tab timeout --tab <ID> --timeout 20m  # measured from command start; none disables
```

Each shell command-start mark starts a fresh deadline; command-finish marks cancel it. Idle shells never expire by this clock. Updating the bound applies to the current command from its original start and to future commands; `none` disables both. Expiry stops the owned process tree by restarting the shell, keeps the tab identity, title, purpose and cwd, and sends one owner `timed out` status with command, command elapsed and deadline. Existing owner/Team cleanup still removes orphan utility shells. `tab restart` retains the bound; `--timeout` overrides it. Daemon restart retains the bound, clears lost running-command evidence and reports it to the owner. `tab inspect`: `utility_deadline` includes `duration_ms` and nullable `started_ms`, `deadline_ms` (epoch ms); idle bounds have no deadline.

REST: `POST /api/tabs` and `POST /api/tabs/{id}/restart` accept optional `timeout`; `POST /api/tabs/{id}/timeout` takes `{timeout}`.

`tab new --cmd` and `tab restart --cmd` return the created or replacement shell id and submission receipt, without waiting for command completion. `command_delivered: true` includes `command_submission` with the exact `pty_id`, `status: submitted`, `bytes_queued` and `evidence: shell_command_start`. Post-write or lost-response uncertainty returns `status: submit_unconfirmed`; never replay that command. Proven non-submission is refused as `shell_command_not_submitted`. Use `tab exec` for final exit status. `tab new/restart --cmd` and `tab exec` reject C0 except tab/CR/LF. JSON paths need forward slashes or doubled backslashes.

## Storage authority and recovery

Desk authority: `<data_dir>/storage/state.db`, opened before restore. Open/import/archive-verification failures leave state unavailable, never replaced by an empty desk. Import preserves recognized sources in `storage/legacy-originals/`.

Backup: stop cleanly; copy all `storage/` (DB, assets, import archive). Restore into empty `storage/` with a Storage-compatible build. Keep backup/originals: legacy files lack post-cutover changes, so an older binary is not rollback.

## The restore contract

1. Tabs retain `agent_session_id`, binding nonces, `role`, `role_profile`, `team_id`, `parent_tab_id`, `can_spawn`, `depth`, model provenance, chatBackend binding and prior lineage, even without a session.
2. PTY resume uses the session's unique provider, proven by its first-turn authoritative record, not a later index. Missing labels repair before visible/lazy launch claims, rechecking identity at commit, retaining session, never replaying tasks. OpenCode validates only explicit `--session`, never latest-for-cwd.
3. Missing/ambiguous ownership retains session but refuses materialization: shell, null provider. Without a claimed session, the saved provider may launch fresh. Another tab, newest-for-cwd or journal never chooses a claimed session's provider.
4. Pre-journal snapshots restore normally. Optional lineage is preserved verbatim for inspection, never admission.
5. Restored counts require observed live agents (reporting below).
6. Armed graceful-shutdown exits retain provider/session/nonces. Failed launches retain provider, role/profile, session, exact restore data and failure card. Only authenticated-launch exit clears identity into lineage. Recovery requires the provider to verify this tab's nonce-bound session or journaled launch target exists for its cwd, then binds a new nonce. `agent restart` resumes in place with saved arguments. `LaunchBindingUnconfirmed` + shell-ready PTY + known provider + no saved/journaled session yields `fresh_launch`. A hand-started provider inherits the exited nonce; its hook with the newest retired nonce adopts `agent_ready` only if no launch owns the tab.
7. A Team member that exited to shell before shutdown stays a shell; report: `agent_exited_before_shutdown`, `exited_session_id`. Recover with `agent restart`.
8. A missing saved cwd never redirects launch: retain cwd/session, show `spawn_failed` naming the directory and notify. Restore the directory, then `domios agent restart --tab ID`.
9. Exact sessions are never silently replaced. Provider-confirmed missing sessions offer **Start new session** / **Keep shell**. `domios agent restart --tab ID --fresh` clears dead session/exact binding, retaining tab/cwd/Team/plan/model/effort. Existing exact sessions refuse; live agents require `--force`.

## Resume after restart

Mid-turn PTY agents continue after quit/update/self-install/crash/kill; no shutdown hook:

- Each live working or idle/question edge persists `turn_resume` with the tab's own `session_id` and observing `start_epoch`. Turn completion clears it; armed graceful shutdown freezes it against teardown.
- An earlier start's unsettled mark gets one Domios system continue via post-ready send at `agent_ready`, nonce-bound to that session. Claim (`settled: {start_epoch, outcome: "sent"}`) persists before write, preventing crash replay. Resumed work marks the following start.
- Different-session/shell/non-PTY/not-ready-within-300-seconds marks settle without retry as `outcome: {"skipped": "<reason>"}` (e.g. `session_not_resumed`, `agent_not_ready`, `send_refused`, `disabled`). Idle tabs have no mark; owner messages never replay.
- Refused or unconfirmed submission emits a system notice and `turn-resume-incident` with code, tab, session and `bytes_written`; settlement is `submit_unconfirmed`, never replayed.
- `update install --from-file` passes `--continue-tab`/`--continue-prompt` to `/api/session/save`. That prompt survives the caller's turn completion and replaces the default continue once; the installer helper never writes to tabs.
- `[agent] continue_after_restart = false` settles owed marks as `disabled` (see `domios docs CONFIG`).

The tab API exposes `turn_resume`; the first build introducing it has no earlier marks.

## Restore reporting and observed liveness

`state/recovery/last_restore_report.json` updates at wave arming/evidence changes; late liveness corrects dropped/unobserved outcomes. One log line/pane/wave. `previous_epoch_end`: `clean_shutdown:<reason>` or `crash:<classification>`.

Backend materializes PTYs on frontend readiness or deadline expiry, never abandons relaunch; expiry records `backend_materialization: "frontend_readiness_deadline_elapsed"`.

Liveness needs live PTY session, real launch transition, own-shell finished-command report or open plugin session, never elapsed time/rebuilt records. Bare shells report role/reason `agent_not_live`; unknown: `agent_liveness_unobserved`.

While startup ACK is owed, owner prompts move `agent_ready` to `awaiting_owner_prompt`, then back on clearance. User-owned prompts notify once/blocker kind/launch with tab/need; parents receive question/options and `domios tab respond` instructions. Owner-launched tabs get no automatic keys.

Observed `absent` panes undergo normal teardown: clear `agent_state`, elapsed timer, status line, `agent_session_id` and model label; arm guarded-input refusal.

## Where the journal lives

Production: `~/.domios/sessions/tab-lifecycle.jsonl`; other instances use their `sessions/`. Agent access: read-only; never edit journal/snapshots.

## Format

One JSON object per line, in journal order: `transition` or compacted `checkpoint`.

Shapes:

- `{record:"transition", entry:{seq, key, transition:{transition, ...fields}, recorded_at}}`
- `{record:"checkpoint", checkpoint:{key, committed_through, authority, agent_type_write, sealed_at}}`
`authority` contains `provider`, `session_id`, `binding_nonce`, `role`, `role_profile`, `team_id`, `can_spawn`, `depth`. Checkpoint `agent_type_write` contains `origin`, `new_provider`, `session_id`, `nonce_verified`, `applied`, `recorded_at`.

### Lineage

`key` identifies one operation: `tab_id`, per-tab monotonic launch `epoch`, and `operation_id` UUID minted with it. Persisted `agent_identity.lifecycle_epoch` / `agent_identity.lifecycle_operation_id` link snapshot to journal for inspection.

### Ordering

Only file-wide monotonic `seq` orders records; UTC `recorded_at` / `sealed_at` never order/select/resolve conflicts. Parse failure/nonadvancing `seq` refuses that line and tail, reports corruption, moves tail to adjacent `tab-lifecycle.refused.jsonl`; never replayed. Exit recovery requires replayable `session_bound`/checkpoint evidence.

### Transitions

Recorded at mutation commit, never inferred from later tab state:

| `transition` | Contract |
|---|---|
| `epoch_opened` | Launch operation begins. |
| `provider_identity_committed` | Provider accepted. |
| `agent_type_write` | Label-write audit: origin, previous/new/attempted provider, caller tab, claimed session, nonce verification, applied. Parser writes are diagnostic only. |
| `session_bound` | Fields: `session_id`, `binding_nonce` (current launch). `source`: `hook` (nonce echoed; default if absent), `pid_registry` (own PTY-tree process record with matched start time), `launch_marker` (session record carries launch's marker). Detected sources run only after hook launch deadline. |
| `role_binding` | Committed role/profile/`can_spawn`/`depth`; absent `role` deliberately clears it. |
| `team_binding` | Committed Team/parent; absent `team_id` unbinds. |
| `launch_ready` | Seals operation. |
| `restart_requested` | Accepted restart; relaunch opens new epoch. |
| `restore_awaiting_agent_restart` | Diagnostic: pre-shutdown exited member not relaunched; never authority. |
| `rolled_back` | Withdraws authority; operation can never seal afterwards. |
| `closed` | Tab gone. |
| `permission_auto_confirm` | Opt-in permission auto-confirm audit: `decision` = `enabled`, `disabled`, `confirmed` (with `tool_name`, `option_label`, `rule`, `question`, `prompt_fingerprint`, `session_id`) or `refused` (with `reason`). Never authority; newest 20/tab retained. |
| `remote_ownership_released` | Diagnostic local-control release, peer and optional caller tab. Never authority; newest 20/tab retained. |

### Checkpoints

Sealed launches with provider/session committed and no rollback or closure; evidence only.

## Retention and compaction

Bounded diagnostic history; compaction preserves surviving evidence.

## Inspecting it

Filesystem inspection: filter `entry.key.tab_id`/`checkpoint.key.tab_id`; read `entry.seq`, `entry.recorded_at`, `entry.transition.transition`.

```bash
domios tab inspect --tab db79d2da --lifecycle
```

Session backups use Storage projections. Restore adds missing workspaces/tabs with original ids; existing entries stay unchanged/open. `tab inspect --tab ID --lifecycle` adds read-only `lifecycle`: live lineage, ordered transitions, surviving checkpoint, tail health. `tab list` / `tab list --workspace` omit it. No journal restore/checkpoint mutation.

## Attempt retirement fences a closed tab's dispatch

Closed attempts cannot become ready/reactivate; recover through Plan owner. Closed tabs or fenced exited Workers holding `in_progress` tasks emit one `[worker lost: <task>]` owner notice and bus `worker-lost` per plan/task/attempt: `plan_id`, `task_id`, `worker_tab_id`, `orchestrator_tab_id`, `worktree_path`, `branch`, `lane` state, `cause`, `recovery`, `delivery`.

No live owner agent: withhold (`delivery: withheld_no_agent`), alert user once with affected tasks, deliver once when owner ready; never type into shell. Attributed `tab close` of such Workers refuses (`409`, `busy worker close refused`) unless `--force`; human close returns `warnings`. `plan doctor`: `in_progress_tab_gone` is error.

## Not in scope

- Tab creation: first journal entry is `epoch_opened`.
- chatBackend lifecycle authority: detached provider-label audits only, no operation/checkpoint.
- Named-checkpoint rollback or journal replay.

## General Agent recovery

Watchdog checks shutdown, launch grace, PTY liveness/restart bounds on launch settlement, startup deadline, PTY death, provider exit to shell, idle or busy without output. Screens never authorize live-PTY restart. Confirmed dead-PTY recovery retains watchdog predicates without a model.

Connected observer ignores confidence below 0.60. Model-only owner notices require two agreeing observations >=0.80; automatic actions require both >=0.90 plus deterministic evidence:

| State | Authority |
|---|---|
| Update prompt | Agent-owned: one calibrated choice >=0.80 among provider-declared options. Owner-launched: owner answers. |
| Crashed to shell | Confirmed exit, provider absent from own process tree, live shell PTY: resume verified session or launch fresh. Jev audit-only. |
| Starting past deadline | One restart; provider absent or PTY dead. |
| Error loop | One restart, then notify; provider absent or PTY dead. |
| Login or unavailable model/quota | Notify owner with blocker-matching choices. |
| Folder trust | Trust responder requires provenance, shared trust cache or live same-provider tab with verified session in exact canonical directory. |
| Command/tool permission | Tab's permission policy; observer never approves. |
| Other | No recovery. |

Automatic model recovery requires separate human-labelled held-out precision 0.95 in the 0.90 band plus recorded live oracle. Each dispatch first journals to Supporter and notifies verbose. Durable maximum: two restarts/General Agent episode despite higher configuration. Exhaustion notifies failure once; owner restart resets episode.

Trust/permission/login notice suppression and restore grouping: `domios docs AGENT-CONTROL`. Observations remain journaled; durable tab/state/launch keys survive app restarts, and new launches may notify again.

Bound-session access refusals immediately raise one `error` / `action_needed` incident/access-loss transition, even UI-created tabs, waiting on authentication. Successful turns clear/resolve it. See `domios docs NOTIFICATIONS`; role escalation: `domios docs ORCHESTRATION`.

## Unattended launch prompts

Visible/hidden owned prompt policy lives in the same document (calibrated >=0.80 choices, structural fallback, owner-only blockers, provenance exceptions, direct owner-launched tabs).

Unresolved prompts route parsed options to owner. User-audience tabs notify once/blocker kind/launch, including parent agent if present. Supporter journals each decision as `owned_prompt_answer`, `decided_by: adapter` or `fallback(<reason>)`, without raw screen text/credentials. Hidden PTYs and screen parsers use 120x40 geometry.

## Provider updates

Production coordinates automatic updates with one verbose summary; named/non-production instances require `updater.allow_provider_updates_in_non_prod = true`.

Update check/classified ManualUpdate prompt runs declared updater in utility process (300-second deadline). Durable cross-instance provider/version claims permit one command/version, one run/provider at a time; interrupted claims prevent reruns. Prompt API uses declared safe option, never Update now in agent tab.

Working tabs continue. Success: idle tabs (General included) `agent restart` as verified `resumed` or eligible `fresh_launch`. Active turns/in-progress attempts await idle/task settlement; maximum one restart/tab/update. Failure: no restarts, one owner failure notice with bounded stdout/stderr.

Summary counts restarted/pending/failed restarts and updates in place as pending tabs finish. Supporter journals update runs, safe prompt answers and individual restart outcomes.

<!-- revision: 66 -->
