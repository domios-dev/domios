# Tab lifecycle journal and unconditional restore

CodeTerm keeps one append-only journal of every authoritative tab lifecycle
transition. The journal is **telemetry**: a forensic record of what each launch
committed and when. Normal session restore does not use it as authority.

**Tab restore is unconditional.** Whatever tabs were open at shutdown come
back. A claimed PTY session re-establishes its provider only from its own
provider session record; without unique ownership the restored tab remains a
shell. Journal state — missing, mismatched, truncated, or absent — does not
change that decision.

## The restore contract

1. `sessions/last.json` is loaded and every tab it holds is rebuilt with its
   persisted `agent_session_id`, binding nonces, `role`,
   `role_profile`, `team_id`, `parent_tab_id`, `can_spawn`, `depth`, model
   provenance, and chatBackend binding intact.
2. A PTY agent tab resolves the unique provider whose store owns that tab's
   claimed session id, then relaunches that provider with the session
   (`--resume`-style continuation; `commands::pty::spawn_pty` owns the choice).
   A missing provider label does not bypass that lookup: a unique owner repairs
   the label before either visible or lazy restoration claims the launch. The
   repair preserves the session and checks the tab's identity again before
   committing; it never replays a historical task. Missing or ambiguous ownership
   retains the saved session and refuses agent materialization. A provider
   proves ownership from its authoritative per-session record, which exists
   from the session's first turn, never only from a secondary index the provider
   writes later: a session that has not reached that index yet still resumes.
3. If a claimed session has no unique provider record, the tab comes back as a
   shell with a null provider label. A snapshot with no claimed session may
   still fresh-launch its explicitly persisted provider. Restore never chooses
   a claimed session's provider from another tab, the newest session for a cwd,
   or the journal.
4. Pre-journal snapshots restore like everything else. Lineage fields are
   optional telemetry, not an admission ticket.
5. A rebuilt record is not a restored agent. `crash/restore_report.rs` counts a
   pane as restored only once an agent process is observed live behind it, and
   the "all N restored" summary is emitted only when every pane is. A pane that
   came back as a bare shell is named with its role and reason
   (`agent_not_live`), and one whose liveness is not yet observable is reported
   as `agent_liveness_unobserved` rather than assumed either way.
6. A provider exit observed while a graceful shutdown is armed (update, quit,
   restart, signal) keeps the tab's provider, session and nonces. The shutdown
   killed that process, so the snapshot written at exit still names the session
   the next boot resumes. Outside a shutdown the same exit clears the identity.

## Restore reporting and observed liveness

`state/recovery/last_restore_report.json` is written twice or more per boot: once
when the wave is armed (records back, nothing observed yet) and again whenever an
observation changes, until no pane is unobserved. It also records
`previous_epoch_end` — `clean_shutdown:<reason>` or `crash:<classification>` —
so a restore that lost every agent to a planned update reads as that, and an
unexplained one cannot hide behind a plausible story.

Restored agent PTYs are materialized by the backend once the frontend reports
ready, and still when it misses its readiness deadline: the deadline is never a
reason to abandon relaunch. The visible and backend paths take the same per-tab
PTY claim, so a late frontend reuses the live PTY. A wave materialized without
the frontend records `backend_materialization:
"frontend_readiness_deadline_elapsed"`, so panes still unobserved name what the
restore was waiting on.

Liveness itself is one observation, `codeterm_types::pty::agent_liveness`. Its
inputs are facts: a live PtyManager session, a boot stage emitted at a real
launch transition, a shell reporting its foreground command with a matching
nonce and then finished (OSC 633 `E` → `D`), an open plugin session. Elapsed
time is never an input.

A pane observed `absent` is reconciled through the ordinary lifecycle teardown,
so it keeps no `agent_state`, elapsed timer, status line, `agent_session_id`, or
model label, and the existing guarded-input refusal arms with it.
In-tab provider relaunch, exit recovery and routed-slash behaviour: `docs/contributing/agent-restart-testing.md`.

Role strings still pass through the literal legacy migration
(`Orchestrator`/`Conductor` → Manager with the orchestrator kind); that is a
format upgrade, not a gate.

## Where the journal lives

| Instance | Path |
|----------|------|
| Production | `~/.codeterm/sessions/tab-lifecycle.jsonl` |
| Dev | `~/.codeterm-dev/sessions/tab-lifecycle.jsonl` |
| Named dev instance | `~/.codeterm-dev-<name>/sessions/tab-lifecycle.jsonl` |

It sits beside `last.json`, `backups/`, and `agent-session-identities.json` in
the same `sessions/` directory.

**Agent access is read-only.** Read, grep, or diff it; never hand-edit the
journal or a snapshot.

## Format

One JSON object per line, in journal order. Two record kinds:

```jsonc
// a committed transition
{"record":"transition","entry":{
  "seq":41,
  "key":{"tab_id":"db79d2da","epoch":3,"operation_id":"6f1c…"},
  "transition":{"transition":"session_bound","session_id":"…","binding_nonce":"…"},
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

A tab's own persisted lineage (`agent_identity.lifecycle_epoch` and
`agent_identity.lifecycle_operation_id`) links its snapshot to its journal
history for inspection. Restore preserves the lineage fields verbatim and
otherwise ignores them.

### Ordering

`seq` is a single monotonic sequence across the whole file. It is the **only**
ordering authority. `recorded_at` and `sealed_at` are UTC stamps for reading the
journal by eye — they are never consulted for ordering, checkpoint selection, or
conflict resolution.

A line that fails to parse, or whose `seq` does not advance, refuses itself and
everything after it: `JournalLoad::tail` reports the refused line and reason so
the corruption is visible instead of silently becoming history. A refused tail
affects only what the journal itself can report — it has no effect on restore.

### Transitions

| `transition` | Meaning |
|--------------|---------|
| `epoch_opened` | A launch operation begins. |
| `provider_identity_committed` | Provider accepted for this operation. |
| `agent_type_write` | Provider-label write audit: origin, previous/new/attempted provider, caller tab, claimed session, nonce verification, and whether it applied. Parser writes are diagnostic only. |
| `session_bound` | Provider session bound and proven by the echoed launch nonce. |
| `role_binding` | Role/profile/`can_spawn`/`depth` as committed. `role` absent means a role deliberately cleared. |
| `team_binding` | Team and parent as committed. `team_id` absent means unbound. |
| `launch_ready` | **Seals** the operation. |
| `restart_requested` | A restart was accepted; the relaunch opens a new epoch. |
| `rolled_back` | Authority withdrawn. The operation can never seal afterwards. |
| `closed` | Tab gone. |

Each transition is written by the code that commits the mutation, in the same
operation that performs it. Nothing observes tab state afterwards and infers a
transition from it. Only PTY transport commits authoritative lifecycle
transitions. Provider-label writes without an open lifecycle operation use a
detached audit key and cannot produce a checkpoint.

### Checkpoints

An operation becomes a **checkpoint** when `launch_ready` sealed it,
`provider_identity_committed` and `session_bound` both landed in that same
operation, and no `rolled_back` or `closed` followed. Checkpoints exist so the
journal can be compacted and so inspection can show what a launch actually
committed — they carry no restore authority.

## Retention and compaction

The journal is bounded by explicit compaction, never by age:

- Compaction triggers automatically past 4096 records, and can be performed
  directly through `TabLifecycleJournal::compact`.
- It rewrites the file as one checkpoint per tab, a bounded 16-entry lineage
  of prior identity checkpoints, the latest provider-label attempt, and the
  transitions belonging to operations no checkpoint covers.
- Closed tabs leave nothing behind.
- Replaying a compacted journal reaches exactly the checkpoints replaying the
  full history reaches.

The shared atomic write (unique temp, fsync, replace) is the only pruning path.

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
# what the snapshot persisted, next to what the journal recorded
jq -r '.workspaces[].tabs[].split_tree
       | select(.agent_type != null or .role != null)
       | "\(.tab_id) role=\(.role) provider=\(.agent_type) epoch=\(.agent_identity.lifecycle_epoch)"' \
  ~/.codeterm-dev/sessions/last.json
```

Rotated snapshots in `sessions/backups/` are plain JSON and diff the same way.
For one selected live tab, `codeterm tab inspect --tab ID --lifecycle` adds a
read-only `lifecycle` object with live lineage, ordered transitions, surviving
checkpoint, and journal-tail health. Default inspect output and `tab list`
stay unchanged. The projection is telemetry only; it has no restore or
checkpoint-mutation surface.

## Attempt retirement fences a closed tab's dispatch

Closing a tab flags every task bound to it (`flag_tab_gone`). That now sets
`retired_at` on the tab's live execution attempts as well as `closed_at`, in the
same write. Retirement is a one-way fence: `finish_task_dispatch`,
`mark_task_attempt_runtime_ready`, and the startup-attempt lookup that backs
`agent ack` all refuse a retired attempt. A `dispatch_ready` that lands after the
tab is gone therefore fails its commit instead of settling the task into
`in_progress` with no tab behind it.

The same fence backs Plan-owner recovery: `retire_stale_task_owner` retires the
stale attempts, clears the dead owner identity, and supersedes the epoch in one
transaction, so the intermediate state is never observable.

## Not in scope

- **Tab creation** is not journaled; the first `epoch_opened` marks its lifecycle.
- **chatBackend lifecycle authority does not exist.** Those tabs may have
  detached provider-label audit records, but no lifecycle operation or
  checkpoint.
- **Rollback to a named checkpoint** is not implemented. The journal records
  history; it does not replay it.

<!-- revision: 21 -->
