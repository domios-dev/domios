# Runtime provider matrix (live LLM bench)

Runs against an already-running DEV through `evals/_lib/lib.sh`. It spends real
provider tokens. The provider needs `agentRuntime`; `PROVIDER=codex` is the initial
selector. Model IDs come from that provider's catalogue. No Rust build is run.

From a Domios agent tab with an AgentPad:

```bash
PROVIDER=codex bash evals/runtime-provider-matrix/run.sh
```

The runner creates one uniquely named provisioned DEV workspace. Its agents,
plan, cron and approval file are disposable. It never selects or sends to Main.
All CLI calls pass through `dc`, HTTP reads through `dev_get`, and HTTP writes
through `dev_post`. The Node driver enters through `dev_run`. Every command has
a failure deadline; state observations use bounded lib reads without a sleep
or timer interval. Default deadline: 60 seconds per operation. Configure
`MATRIX_DEADLINE_SECS` before the run when an operator owns a restart window.

Rows print PASS, FAIL or explicit N/A with their evidence. N/A means the surface
is unavailable or cannot be confined on shared DEV; it does not claim success.
An unknown capability fails; an explicitly unsupported prompt-answer capability
is N/A. Unsupported subcases are independent so they cannot mask supported
checks. Ordinary runtime refusals and missing readiness still fail the run.

Assistant observations require new message IDs and assistant type, followed by
idle and a fresh final transcript read. A matching user echo is insufficient.
Raw command responses and exits are retained in numbered JSON files under
`target-check/evals/runtime-provider-matrix/<run>/`, with results JSON and a
Markdown table. Failure exits 1; cleanup failure also fails the run.

| Row | Observable oracle |
|---|---|
| 1 | Bare spawn system codeword round-trips through CLI send into a new assistant message |
| 2 | Operator restarts DEV; process identity changes; neutral question retrieves the original system fact |
| 3.native | Restored native tab retains session ID, chat backing and nonempty history |
| 3.legacy | N/A: no workspace-scoped legacy snapshot import API exists |
| 4 | CLI, self-turn and peer-turn model changes each appear in workspace-scoped tab list |
| 5 | Worker task is in_progress; name, executor role and opaque guidance return |
| 6 | Planner spawned with role and plan returns Planner |
| 7.orchestrator | Owned Orchestrator returns its actual role |
| 7.general | N/A: shared DEV General Agent is outside bench ownership |
| 8 | New compaction timeline item is observed before repeating role questions |
| 9 | Resume retains thread and identity; forced fresh changes thread and retains identity |
| 10 | Working long turn is interrupted; idle returns on the same thread |
| 11.cron | One-shot cron persists its report |
| 11.hidden | N/A: hidden-worker spawn falls back to the active/first workspace |
| 11.dreamer | N/A: dreaming cannot be confined to owned memory/provider on shared DEV |
| 12 | Owned read-only approval probe waits for a bound native request; turn-only answer produces exact owned file content |

## Operator restart

Shared DEV is never restarted by this script. Before starting a run, select a
new marker path and give the operator the checkpoint:

```bash
MATRIX_RESTART_CHECKPOINT=/tmp/runtime-matrix-restarted \
  MATRIX_DEADLINE_SECS=900 PROVIDER=codex \
  bash evals/runtime-provider-matrix/run.sh
```

Row 2 writes `<marker>.ready` with its owned tab, session, history and DEV PID.
The operator restarts DEV through the documented canonical dev procedure, waits
for real health readiness, then creates `<marker>`. The runner requires a changed
PID and reads the restored tab. A pre-existing marker fails. Elapsed time never
counts as restart success. The operator removes marker files.
Without a checkpoint row 2 fails and row 3.native records missing restart.

An interrupted hand probe can instead resume with `MATRIX_RESUME_FIXTURE=<json>`.
The fixture records owned workspace, tab, session, binding nonce, build SHA,
codeword, exact assistant ID and provenance. The runner requires that exact
assistant item to survive, then requires a changed build and a loaded runtime
with the same session before asking the neutral question. This mode covers
restarts onto a different build. Treat these files as historical run artifacts,
not canned PASS data. A cleaned fixture must not be reused.

## Approval probe

Current `agent spawn --help` has no runtime/access-mode flag. The Agent Spawn
contract in `prod-docs/API-AGENTS.md` derives spawn access from global
`agent.full_access`; this bench never changes that setting or adds a spawn flag.
The operator approved this documented tab-local alternative:

1. Spawn a bare owned Chat probe without launch work.
2. Before its first work turn, POST `/api/tabs/{id}/agent/model` with its observed
   model and `runtime_mode: "approval-required"` through `dev_post`.
3. Ask it to execute one Node command writing the opaque codeword to a unique
   file inside the provisioned workspace, using native approval when required.
4. Require a native prompt whose request token matches the current runtime epoch
   and binding nonce, and whose action names that exact file and codeword.
5. Confirm the same thread and approval-required mode, with no file written yet.
6. Answer the provider's offered Approve/Allow once/Grant for turn option through
   `dc tab respond`. Session-wide grants are never selected.
7. Require idle and exact persisted file content, then remove the owned file.

For Codex, the provider policy maps approval-required to untrusted approvals
with a read-only sandbox (`plugins/provider-codex/src/runtimeSettings.ts`).
That is a source fact until this probe is live-run. The earlier full-access
question probe did not exercise approval and its failed result remains recorded.
If `supports_prompt_answers` is explicitly false, row 12 prints N/A with the
provider capability evidence and creates no probe.

## Shared DEV limitations and post-release hooks

The session surface exposes save, backups list and additive full-desk backup
restore (`prod-docs/API-STATE.md`, `src-tauri/src/api/routes.rs`). There is no
workspace-confined snapshot import route. Full-desk restore can recreate foreign
workspaces and cannot seed only an owned legacy tab; this bench never calls it.
Post-release proposal: a DEV-only typed import that accepts a bench-owned
workspace and legacy tab snapshot, refuses foreign/protected IDs and Team
bindings, and returns imported tab/session evidence before the restart checkpoint.

General Agent is instance-scoped. Post-release proposal: the operator provisions
a named isolated DEV instance with disposable Storage and General identity, then
supplies an ownership receipt binding the bench to that instance. The bench can
use the lib's existing named-instance selection; no lane dev server is started.
Its General identity and dreaming checks must use only that isolated instance.

Dream run has no provider/workspace override (`codeterm mem dream run --help`).
The isolated-instance hook must also select the requested provider and seed only
synthetic owned memory, then require a completed dream report before teardown.

Hidden-worker `--parent` only records provenance. Its spawn spec has no
workspace_id (`src-tauri/src/mem_host/worker_spawn_spec.rs`), and hidden launch
falls back to active_workspace_id or the first workspace
(`src-tauri/src/service/spawn.rs`). Post-release proposal: explicit owned-workspace
selection for hidden workers, or the same isolated-instance ownership hook.
Changing global focus is not a confinement mechanism. Row 11.hidden is N/A
until one of these hooks exists.

## Verification

Historical live results are in [RESULTS.md](RESULTS.md). The next live run waits
for the operator's READY after the Hitch, Dial, Key and Phoenix merges, with an
operator-triggered restart checkpoint. No live rerun was performed while updating
these N/A policies and the approval probe.

Source-only checks:

```bash
bash -n evals/runtime-provider-matrix/run.sh evals/runtime-provider-matrix/bridge.sh
node --check evals/runtime-provider-matrix/matrix.mjs
node --test evals/runtime-provider-matrix/observations.test.mjs
```
