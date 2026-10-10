# macOS named-instance trial

This lane also supports the authorized aiderqa instance on port 7697. Keep the
application checkout on the Orchestrator-approved scored build while running
the trial. Start only that DEV:

```bash
CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697 ./build.sh dev --wait
CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697 bash evals/restart-endurance/run.sh \
  --out target-check/evals/endure-mac-trial-1 --notice-hook /absolute/path/to/notice.sh
```

Named trials create only fresh/used provider fixtures in Endure. They require
Claude, Codex and OpenCode, use deepseek/deepseek-flash for OpenCode, and include
Grok when installed. They leave General siblings and setup alone. Empty fresh
sessions may acquire a new session id at the first restart; continuity becomes
mandatory after the fixture completes its first answer.
Set ENDURE_WORKSPACE_NAME to a distinct workspace name for a scored trial after
a setup rehearsal; prior fixtures and their evidence remain preserved.

Every restart waits for all created fixtures to reach agent_ready, then probes
concurrently with one 120-second answer deadline per tab. A parallel failure
retains its verdict, timestamps and health latency; one serial retry is recorded
separately. Grok answers allow a marker prefix to tolerate its output suffix;
a submitted Grok probe is never sent again in the same round.

When owner login is pending, setup can defer Grok with ENDURE_DEFER_GROK=1.
After credentials are available, repair-setup OUTPUT_DIR PROVIDER observes an
existing fresh fixture and seeds an unanswered used fixture, spawning missing
fixtures through the normal path. Original failures remain in results.json and
recovery observations are appended. An explicit PROVIDER:FRESH_TAB argument can
journal an interrupted fixture creation after checking its provider, workspace
and cwd. It never approves trust dialogs or restarts failed agents.

Use --continue-from-setup only after reviewing a completed setup before the
first launcher invocation or restart round. A pre-launch harness failure is
archived with its original manifest and retained in harness_failures; the
continuation keeps completed seeds and never repeats setup prompts. Any launcher
log or restart-round evidence refuses continuation.

After round 3, discover alternate models with dc agent models PROVIDER, then run
these observers through the named DEV library:

```bash
source evals/_lib/lib.sh
dev_run node evals/restart-endurance/observe.mjs switch OUTPUT_DIR PROVIDER:MODEL
```

Switch proof requires the same session and the next completed answer's persisted
provider model, read from the exact session's Claude JSONL, Codex rollout,
OpenCode SQLite assistant row, or Grok updates.jsonl. The model observer appends
model_switches to results.json. Node with node:sqlite support is required.

The external notice hook uses the assigned Orchestrator's return route. When the
Orchestrator requests no progress messages, retain the notice locally instead.
Keep all authored messages in the caller's AgentPad. Stop only
aiderqa after evidence collection; retain the worktree and trial artifacts.

# Restart endurance (live LLM eval)

Run only in the canonical checkout, in one Team-bound QA utility shell, under
the Orchestrator's serialized QA authority. This spends provider tokens and
restarts the shared DEV instance three times. Production port 7685 is never a
restart target. No application source or global configuration is changed.

```bash
bash evals/restart-endurance/run.sh \
  --out target-check/evals/endure-trial-1 \
  --model opencode=deepseek/deepseek-flash \
  --notice-hook /absolute/path/to/restart-notice.sh
```

Windows: invoke the same command through Git Bash from PowerShell. Node.js,
Python 3, Git Bash/bash, and the active DEV CLI are required. `ENDURE_BASH` is
resolved to the actual invoking Bash executable on Windows.

Repeat `--model provider=model` to pin fixture models from the provider's
catalogue. Both fresh and used fixtures receive the same override; omitted
providers keep their configured default, except named macOS trials default
OpenCode to deepseek/deepseek-flash. Explicit overrides also apply when creating
missing fixtures with repair-setup. The manifest records the effective overrides.

## Preconditions and coordination

DEV 7686 must already be healthy, built from canonical HEAD, and have a completed
General Agent setup. If necessary, the Orchestrator first runs the authorized
`./build.sh dev --wait --wait-build` in its Team-bound utility shell. The eval
checks health identity and build SHA before fixture creation and each round.

The runner uses `./build.sh dev-prebuilt --wait --wait-build` for the three
rounds. That owning command validates the saved app/CLI/frontend provenance
before its DEV-only kill and starts the same binary with a bounded health wait.
It runs no Cargo rebuild when provenance matches. A stale-provenance refusal
before DEV is stopped uses the incremental `dev --wait --wait-build` fallback;
other launcher failures stop the trial. Authorize both lifecycle commands for
the default runner. A caller-mandated launcher instead uses the observer's
`setup`, `before`, `round`, and `finish` phases with the same output directory.

`--notice-hook` is a required, caller-owned Bash file **outside `evals/`**. It
runs in the QA shell's original parent connection, before every restart, with
the round number as its only argument. It must write a one-line notice into its
own AgentPad and send it to Fermi (tab `e3ec9ee9`) through that parent connection.
The hook must exit nonzero on delivery refusal; the runner then stops before
restarting. Its output is retained as `notice-N.log`. Configure parent routing
as appropriate for the actual QA shell; do not pass a PROD connection into the
DEV wrapper. If the parent is PROD, explicitly log that this is PROD
**coordination only** before that external hook's CLI send. No production
lifecycle operation is authorized. The eval itself invokes only `dc` and
`dev_run` from `evals/_lib/lib.sh`.

The hook is the integration point for the Orchestrator's existing return route;
the eval does not guess whether a parent tab lives on DEV or PROD. The hook
is bounded at 30 seconds by the runner and must not ask an interactive question.

## What the procedure does

1. Creates a provisioned DEV workspace named `Endure`. Discovers provider
   descriptors and launches roleless, non-user-facing codex, claude, opencode,
   grok, and optionally aider tabs twice, without a launch task. No Workers or
   source-writing briefs are dispatched. Missing required providers fail
   coverage; missing Aider is explicitly optional. Aider is tested only when
   advertised as an actual provider, never substituted with a utility worker.
2. Creates two fresh General siblings and two used General siblings through
   `POST /api/agents/general/sessions`, with the exact camelCase
   `fresh_general` request from `src/lib/generalSession.ts`. This API requires
   the configured provider/cwd and the protected **General workspace**;
   therefore the siblings reside in General and are tracked by the eval
   manifest alongside Endure. Main is preserved.
3. Checks that fresh tabs have no user or assistant conversation turn. Gives
   each used fixture one user turn, `Reply with exactly ENDURE_<provider>_0`,
   and requires an exact answer in that tab's own appended transcript window.
   A fresh fixture that already contains turns is a recorded setup failure,
   not quietly treated as empty.
   Sessionless fresh tabs are valid when their installed provider manifest
   declares `spawn.sessionStartsOnFirstUserTurn` and structural composer
   readiness. The eval discovers that manifest through the DEV plugin inventory;
   it uses no provider-name exception. `agent_ready`, an owned PTY/binding nonce,
   and no pending prompt authorize the first probe. After sending, the eval
   requires a bound session before starting transcript streaming and accepting
   the exact answer. It saves that session and requires it unchanged before
   every subsequent probe. A fresh empty session may receive a new ID on
   restore; continuity starts with its first persisted probe turn. Phase and
   send-response fields distinguish a probe
   that never reached send from a sent turn awaiting binding or an answer.
4. Announces and performs three provenance-checked DEV restarts. After each,
   focuses each fixture in its workspace to materialize lazy PTYs, waits for every
   fixture's agent_ready launch stage, then probes default-instance fixtures
   serially or named macOS fixtures concurrently with the round marker.
   A final authoritative snapshot can reconcile a missed readiness event;
   the original stream gap remains in the result. Unresolved readiness failures
   stop the all-fixture barrier and receive neither another wait nor a probe.
   Waits use an SSE subscription established before sending, a single
   120-second failure deadline per observation, and event-triggered snapshot
   reads. There are no fixed-delay warmups. Default-instance probes are not
   retried; named macOS failures retain one separately recorded serial retry,
   except a submitted Grok prompt is never replayed. Sends read their
   authored prompt from the target tab's private pad and use the user send API,
   avoiding an agent reply-route wrapper around the exact-answer request.
5. Records identity, boot stage, failure reason/prompt, session before/after,
   transcript, elapsed time, lifecycle inspection and tab-specific log evidence.
   Requires provider identity and existing sessions to remain unchanged;
   duplicated session bindings also fail. Checks pre-existing tabs from General
   and Rcqa after each round, read-only, because authorized writes are confined
   to the created fixtures. Those rows say `OBSERVED`, never claim probe PASS.

A transcript pass requires a new exact user turn after the baseline and an
exact assistant answer within that user turn. History, typed echo, thinking,
tool output, extra answer text, and replies to subsequent user turns cannot
satisfy it. `reply_ms` includes restore readiness and send time; `elapsed_ms`
also includes the failure path, while lifecycle inspection follows afterward.
The probe also requires reported idle status or an explicit completed-turn
record after its pre-send boundary in the exact bound provider session before
starting the next restart. Stale working status remains in the evidence.
The oracle does not derive provider completion from screen text or automatically
answer authentication prompts.

## Artifacts and exit codes

- `results.json`: incremental machine-readable manifest, setup round 0,
  rounds 1–3, full observations and provider availability.
- `results.md`: one table per round and a section for every failed fixture.
- `restart-N.log`: launcher/provenance/health output.
- `notice-N.log`: evidence that the notice hook completed before restart.
- `round-N.log`: new restore/launch/identity-clear/failure lines from DEV logs,
  including retained rotated files. Line hashes captured before restart exclude
  old evidence without assuming a log timezone. Tab-specific lines are copied
  into each row; absent causal evidence is explicitly unproven.
- `complete`: final exit code, 0 for full fixture coverage and successful probes,
  1 for observed coverage/fixture failures. Harness/startup refusal exits 2.

Repeated invocation against a completed output directory prints the same
receipt and returns the saved verdict without creating tabs, sending messages
or restarting DEV. An incomplete run is preserved and refuses automatic replay
of uncertain sends. A directory lock prevents simultaneous writers. A new
trial refuses an already-existing Endure workspace: review the prior manifest
and explicitly close only its workspace and owned General sibling IDs first.
The eval leaves fixtures intact for diagnosis; it never deletes Main, General,
Rcqa or foreign tabs. Keep each trial's output directory for independent proof.

## Lane checks and canonical gate

No DEV run is executed in the source lane. These checks need no Rust build or
running daemon:

```bash
bash -n evals/restart-endurance/run.sh
node --check evals/restart-endurance/observe.mjs
node --test evals/restart-endurance/oracle.test.mjs
node --test evals/restart-endurance/logs.test.mjs
```

Exercise the failed pre-restart log capture offline, using a saved trial and
the readable DEV log directory:

```bash
bash evals/restart-endurance/run.sh --dry-run \
  --out target-check/evals/endure-dry-run \
  --from-results /absolute/path/to/endure-trial/results.json \
  --log-dir "$HOME/.codeterm-dev/logs"
```

This branch runs before loading the DEV library. It makes no CLI or API call,
executes no notice hook or lifecycle command, creates no tabs and sends no
messages. It exercises the same log reader, baseline hashing and evidence
filter used immediately before the restart and on completion. It writes a
separate `dry-run.json` receipt and `dry-run-evidence.log`, retaining round-0
failures from the input manifest. It cannot resume or rewrite the original
trial. Its proof covers log capture, not a successful live restart.

The large-log regression uses 250,000 lines and rotated logs. Appending each
line avoids JavaScript's function-argument limit; spreading an entire file
into `push` previously produced `Maximum call stack size exceeded` before
DEV was stopped. This failure was argument-stack exhaustion, not recursion.

After a real merge, the canonical live gate is the invocation above, with a
working bounded notice hook and the exact `dev-prebuilt` lifecycle authorization.
Pass requires every available created fixture to retain identity and its
existing session, produce the exact round answer in all three rounds, and all
four required providers to be covered. Round-0 failures remain failures even if
a later restart recovers the tab. Analyse retained evidence to distinguish an
observed death, launch refusal, owner prompt, lost identity/session, and an
answer deadline; a deadline alone does not establish that a tab died.
