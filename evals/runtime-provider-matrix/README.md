# Runtime provider matrix (live LLM bench)

Runs against an already-running DEV through `evals/_lib/lib.sh`. It spends real
provider tokens. The provider needs `agentRuntime`; `PROVIDER=codex` is the initial
selector. Model IDs come from that provider's catalogue. No Rust build is run.

From a Domios agent tab with an AgentPad:

```bash
PROVIDER=codex bash evals/runtime-provider-matrix/run.sh
```

The runner creates one uniquely named provisioned DEV workspace. Its agents,
plan and cron are disposable. It never selects or sends to the owner's tabs.
All CLI calls pass through `dc`, HTTP reads through `dev_get`, and HTTP writes
through `dev_post`. The Node driver enters through `dev_run`. Every command has
a failure deadline; state observations use consecutive bounded lib reads without
a sleep or timer interval. Default deadline: 60 seconds per operation. Configure
`MATRIX_DEADLINE_SECS` before the run when an operator owns a restart window.

Each row prints PASS or FAIL and the evidence read. Assistant observations require
new message IDs and assistant type, followed by idle state. A matching user echo
is insufficient. Raw command responses and exits are retained in numbered JSON
files under `target-check/evals/runtime-provider-matrix/<run>/`, with a results
JSON and Markdown table. Failure exits 1; cleanup failure also fails the run.

| Row | Observable oracle |
|---|---|
| 1 | Bare spawn system codeword round-trips through CLI send into a new assistant message |
| 2 | Operator restarts DEV; process identity changes; a neutral question retrieves the original system fact |
| 3 | Restored native and legacy tabs retain session IDs, chat backing and nonempty history |
| 4 | CLI, self-turn and peer-turn model changes each appear in workspace-scoped tab list |
| 5 | Dispatched Worker task is in_progress; name, executor role and opaque guidance return |
| 6 | Planner spawned with role and plan returns Planner |
| 7 | Orchestrator and owned General fixture return their actual roles |
| 8 | New completed compaction item is observed before repeating role questions |
| 9 | Resume retains thread and identity; forced fresh changes thread and retains identity |
| 10 | Working long turn is interrupted; idle returns on the same thread |
| 11 | One-shot cron and hidden worker persist their reports; isolated dreamer reports |
| 12 | Native question becomes pending; numbered response produces selected-color final answer |

## Operator restart

Shared DEV is never restarted by this script. Before starting a run, select a
new marker path and give the operator the checkpoint:

```bash
MATRIX_RESTART_CHECKPOINT=/tmp/runtime-matrix-restarted \
  MATRIX_DEADLINE_SECS=900 PROVIDER=codex \
  bash evals/runtime-provider-matrix/run.sh
```

Row 2 writes `<marker>.ready` with its owned tab, session, history and DEV PID.
The operator restarts DEV using the documented canonical dev procedure, waits
for real health readiness, then creates `<marker>`. The runner requires a changed
PID and reads the restored tab. A pre-existing marker fails. The bench never
interprets elapsed time as restart success. The operator removes marker files.
Without the checkpoint row 2 fails explicitly and row 3 records missing restart.

An interrupted hand probe can instead resume with `MATRIX_RESUME_FIXTURE=<json>`.
The fixture records owned workspace, tab, session, binding nonce, build SHA,
codeword, exact assistant ID and provenance. The runner requires that exact
assistant item to survive, then requires changed build and binding nonce with
the same session before asking the neutral question. This mode only covers
restarts onto a different build. Treat these files as run artifacts, not canned
fixtures or synthetic PASS data.

## Remaining coverage boundaries

This initial lane is partial: it executes the available runtime surfaces and
fails missing preconditions honestly. It does not yet supply these adapters:

- Legacy restore needs an owned legacy PTY tab seeded before the checkpoint.
  `MATRIX_LEGACY_TAB` and `MATRIX_LEGACY_SESSION` assert the resulting tab but
  do not create or prove a legacy pre-state. No shared Storage is edited.
- General Agent creation is not workspace-scoped. `MATRIX_GENERAL_TAB` is
  accepted only inside the owned workspace and without protection. The shared
  protected General tab is never queried or driven.
- Dreaming has no provider/workspace override. Row 11 runs an explicitly selected
  hidden provider worker with a read-only reporting task; it fails the dreamer
  portion rather than consolidate shared memory. An isolated-instance adapter
  is still required for actual dreaming proof.

These gaps fail; they are never skips or passes. Role delivery may fail until
Crest task_v3noch lands; resume/fresh may fail until Phoenix task_ybwrsf lands.
Re-run on canonical DEV after those merges. A source implementation or accepted
RPC alone does not prove any runtime behavior.

For source-only verification:

```bash
bash -n evals/runtime-provider-matrix/run.sh evals/runtime-provider-matrix/bridge.sh
node --check evals/runtime-provider-matrix/matrix.mjs
```
