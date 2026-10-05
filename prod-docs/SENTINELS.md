# Sentinels

A sentinel watches external status cheaply while an agent does useful work. It runs a bounded shell probe at the
requested cadence, hashes the captured result, and classifies only changed output. Use it to wait for CI or build
completion instead of repeatedly asking an agent to run the same status check.

## When to use a Sentinel graph

Before you repeat a check or wait on an external, agent or cron status, describe the loop once as a graph instead of spending turns on it. Unchanged output costs no model call and no agent turn; you wake only when a declared bucket routes to you. Observation and actions are deterministic, the model only picks one of your finite buckets (never a command, target or reply), and every action stays in the creator's Team, workspace and machine. `other` means no bucket fits.

1. **CI or PR result.** `--probe "gh run view ID --json status,conclusion"`, buckets `passed,failed`, running CI left
   to `other`; `--route passed=cron:<review>` and `failed=send:<fixer tab>`. The branch is decided once, no polling.
2. **A Worker waiting on a prompt.** `--observe tab:<worker>` with a `prompt` bucket routed to `notify` or
   `send:<owner tab>`, and `other` escalated. The tab is read, never typed into; its owner wakes only on a stop.
3. **Cron run failures.** `--observe cron-run:<name>`, buckets `ok,failed`, `failed=notify`. Each run is a fresh
   episode, so one graph covers the schedule; the transition count spans runs, so raise `--max-transitions` (max 256).
4. **Deploy or service health.** `--probe "curl -fsS --max-time 2 https://HOST/health" --every 1m` with
   `--match 'healthy=(?m)^exit=0$' --match 'down=(?m)^exit=[1-9]'` and `down=notify`. Only a change of bucket routes,
   not every probe. Stay under the probe limit: a timed-out probe is a skip, not `down`.
5. **Long build or test.** Have the job write `passed` or `failed` to a status file; `--probe "cat out/test.status"`
   with `--match 'passed=(?m)^passed' --match 'failed=(?m)^failed'`. While the file is missing the result is a quiet
   `other`; the default `notify` wakes the creator once on completion, instead of re-reading a log each turn.
6. **Self-improving loop.** Escalate `other` to the owning agent (`escalate:agent:<tab>` in `--handlers-file`). It
   reads `sentinel history`, adds the missing bucket, pattern or handler with `sentinel update ... --cause
   TRANSITION_ID`, then resolves the escalation so the stage re-evaluates once. Each unknown case costs one turn, then
   stays a versioned, deterministic branch.
7. **Watching a whole Team.** A General Agent probes existing state, e.g. `--probe "codeterm orchestrator status
   --plan <plan> --tab <orchestrator-tab>"`, with buckets `healthy,stuck,needs_owner,lost` in `--handlers-file`:
   `"healthy": []` does nothing; `stuck`, `needs_owner` and `lost` each `notify` the creator once per change; `other`
   escalates to `creator`. A `send` target must share the creator's Team, so the General Agent relays `stuck` to the
   Team Orchestrator once with `codeterm send` and takes `needs_owner` to the owner.

## Create a watch

```bash
codeterm sentinel create --probe "gh run view 456 --json status,conclusion" --question "waiting while CI is in progress; finished when CI has completed with any conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h
```

The first probe runs immediately. `--every` defaults to `20s` and must be at least `10s`. Durations accept `s`, `m`, `h`, `d`, and combinations such as `1h30m`; a bare number means minutes. The maximum duration is 366 days. `--timeout` bounds the lifetime. `--until` names one declared bucket and ends the watch after that bucket's first routed transition. `--route bucket=stop` ends it without sending a message.

For quiet PR waiting, declare only terminal outcomes; classify still-running CI as `other`, and add an `other`
escalation handler when it needs attention. With an existing same-Team cron and agent:

```bash
codeterm sentinel create --probe "gh run view 456 --json status,conclusion" --question "passed for completed successful CI; failed for completed unsuccessful CI; other while running" --buckets passed,failed --route passed=cron:review_pr --route failed=send:repair_worker --timeout 1h
```

The creator's live tab supplies its cwd, workspace, and Team. Commands are executed as the daemon user in that cwd, with
no interactive stdin. Each probe has a three-second runtime limit (ten seconds on Windows, where PowerShell starts
slowly), or the remaining lifetime if shorter. Capture retains up to 32 KiB each of stdout and stderr. The exit status
is part of the hashed result. A probe timeout or spawn failure records a skip.

Use `--observe` to watch an agent tab or a cron run instead of running a shell probe:

```bash
codeterm sentinel create --observe cron-run:nightly --question "identify the current run state" --buckets waiting,finished --every 20s --timeout 1h
```

The observed tab must be an agent in the creator's Team and workspace. A `cron-run:<id-or-name>` stage waits for a run,
then observes the exact tab recorded for that run. A finished run or removed tab unbinds the stage; a replacement run
starts a fresh observation episode by resetting the observation hash, accepted bucket, and held state. The transition
count is retained across runs.

## Classification and transitions

A connected Supporter adapter with Choice capability gets one question over `--buckets` and the automatic `other`
option, using the shared request budget and journal. Probe output is treated as untrusted data. Unchanged output never
calls the model, including after a failed or budget-exhausted classification.

Without a connected Choice adapter, supply deterministic patterns:

```bash
codeterm sentinel create --probe "check-ci" --question "CI result" --buckets waiting,passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --every 20s --until passed --timeout 1h
```

Patterns see the formatted result: exit status, stdout, then stderr. Match the status line with a multiline pattern such
as `(?m)^success$` when the probe prints `success`. Exactly one matching bucket is accepted at confidence 1; zero or
ambiguous matches become `other`. Create without either a connected Choice adapter or a pattern is refused.

Routes run once when a new accepted bucket differs from the previous accepted bucket, including the first accepted
result. Model confidence must be at least 0.80. `other` and low confidence can use the reserved `other` handlers;
adapter failure and exhausted budgets never escalate. Unknown or weak observations do not reset the last accepted
bucket. Returning to a previously accepted bucket after another accepted bucket is a new transition.

## Closed-set routes

Repeat `--route bucket=action` for inline `notify`, `sentinel`, `send`, `cron`, or `stop` handlers. Use
`--handlers-file` for `command`, `respond_prompt`, or `escalate` handlers and when explicit handler IDs are needed.
Unassigned buckets use `notify`.

| Action | Result |
|---|---|
| `notify` | Sends the result to the creator agent; a shell creator gets a notification record |
| `sentinel:<exact-id>` | Activates an existing dormant stage owned by the same creator, Team and workspace |
| `send:<tab>` | Sends a result-file pointer through the existing tab message API |
| `cron:<name>` | Triggers the uniquely named existing cron |
| `respond_prompt:<kind>` | Requests an answer through the existing provider-declared safe-response path |
| `stop` | Stops the sentinel |
| `escalate:creator` or `escalate:agent:<tab>` | Sends a small escalation packet through the message gateway |

A send target must be an agent in the creator's Team and workspace, so probe text cannot become shell input. Result
bodies live in the recipient's AgentPad. Cron targets must belong to the same Team and workspace. Targets resolve at
creation and are checked again at routing. No route executes a command chosen by the model. Each accepted transition and
handler attempt has a durable identity. A claim is saved before its action; claimed handlers are never replayed after
restart. Independent successes, failures, skips and unconfirmed outcomes stay visible. Completion confirms the selected
operation, not recipient work or cron success. A failed handler does not erase earlier results or prevent later
handlers; stop cancels the remaining handlers. A missing result after a crash is unconfirmed.

`respond_prompt:<kind>` requires an `--observe` tab or cron-run source. The kind identifies a requested safe-response
path; it does not authorize an answer or select an option. For example, `permission` can request a response to a
per-tool permission prompt. The existing responder revalidates the exact current prompt and kind, then chooses only a
provider-declared option through the existing safe-response writer. Changed prompts, unsupported kinds, unsafe targets,
and prompts without a matching declared option are refused. Sentinel does not write raw PTY keys or construct provider
steps. Each tab accepts at most one response per prompt occurrence, and Sentinel prompt-response requests are limited
to 600 per rolling 60-second window.

## Compose a named Sentinel graph

Create downstream stages first with `--paused`, then create the named entry whose outputs activate their exact IDs.
Paused stages do not probe or spend Choice requests. Their timeout starts at creation and continues while dormant.

```bash
codeterm sentinel create --name "Review stage" --paused --probe "check-review" --question "done when review completes" --buckets done --match 'done=(?m)^done$' --until done --timeout 1h --json
codeterm sentinel create --name "CI and review" --probe "check-ci" --question "passed or failed CI" --buckets passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --route passed=sentinel:sentinel_DOWNSTREAM_ID --route passed=notify --route failed=stop --timeout 1h
```

Replace `sentinel_DOWNSTREAM_ID` with the exact ID from the first response. The entry and its reachable stages appear as
one named scenario. Shared targets retain their identity and results. Activating an already active stage is a noop: it
does not reset its accepted result, schedule, expiry or running work. Missing, foreign, self, stopped and expired
targets are refused. There is no rearm API, recursive scheduler execution, or cyclic-episode guarantee.

Use `--handlers-file PATH` for explicit IDs and JSON bucket-to-array definitions:

```json
{"passed":[{"id":"review","type":"sentinel","parameters":{"target":"sentinel_DOWNSTREAM_ID"}},{"id":"notice","type":"notify","parameters":{}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```

The file form cannot be mixed with `--route`. Unknown fields or types, duplicate IDs and ambiguous targets are refused.
At most 16 handlers fit each bucket and 64 fit one node. Composition definitions require `--timeout`. Each graph has
a lifetime transition budget that defaults to 64 and can be set up to 256 with `--max-transitions`; a new cron run
does not reset that count. Exhaustion stops the graph with `transition_budget_exhausted`. The action-attempt budget
defaults to 256 and `--max-actions` allows up to 1024. No model output constructs a handler, target or command. A fixed local
command is available only through `--handlers-file`, with creator-authored parameters:

```json
{"passed":[{"id":"marker","type":"command","parameters":{"command":"printf marker > marker.txt","label":"Write marker","timeout_ms":1000}},{"id":"review","type":"sentinel","parameters":{"target":"sentinel_DOWNSTREAM_ID"}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```

Use the same file form to declare a prompt answer:

```json
{"ready":[{"id":"answer_permission","type":"respond_prompt","parameters":{"target":"permission"}}]}
```

Save the JSON as `handlers.json` in your disposable fixture directory, replace the downstream ID, and create from that
directory:

```bash
codeterm sentinel create --name "Marker and review" --probe "printf passed" --question "passed or failed" --buckets passed,failed --match 'passed=(?m)^passed$' --match 'failed=(?m)^failed$' --handlers-file handlers.json --timeout 1h
```

Put escalation actions only in the reserved `other` array, for example:

```json
{"other":[{"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}}]}
```

It accepts only `notify` and `escalate` handlers, with at most one escalation. An escalation packet carries only
Sentinel and transition IDs, never probe output. Creating a graph with `other` handlers requires positive
`--escalation-timeout-ms`; updates must set a positive `escalation_timeout_ms` in the definition JSON. After an
escalation the stage holds without classifying until resolution, a new run binding, or timeout (journaled as
`escalation_expired`). Resolve an open episode with:

```bash
codeterm sentinel escalation resolve SENTINEL_ID TRANSITION_ID
```

Resolution clears the observation hash and re-evaluates once. An escalation claim is persisted before delivery and is
never replayed after restart. The same cause escalates at most once per five minutes per Sentinel.

Update a definition from JSON with the current version. Add `--cause TRANSITION_ID` when the amendment addresses an
escalation. Any caller in the same Team and workspace may amend it; each accepted update records version lineage:

```bash
codeterm sentinel update SENTINEL_ID --expect-version VERSION --from-file definition.json
codeterm sentinel update SENTINEL_ID --expect-version VERSION --from-file definition.json --cause TRANSITION_ID
codeterm sentinel history SENTINEL_ID
```

Command text is immutable, nonempty, at most 8192 bytes and contains no NUL. Its display label is at most 256 bytes.
`timeout_ms` is positive and no larger than the platform probe limit. Execution uses the shorter of that duration, the
remaining node lifetime and the platform cap, reserving cleanup once. The authenticated creator cwd supplies the working
directory; there is no handler cwd or environment option. The shell interprets ordinary syntax that the creator wrote.
The host never substitutes observation/model output into commands. Process containment is not a filesystem sandbox:
commands can access files available to the daemon account.

Each command claim is persisted before launch and is never replayed, including an uncertain crash. Success requires
actual zero exit and confirmed parent, boundary and capture settlement. Nonzero, API/spawn error, cancellation, deadline
and unconfirmed resources remain distinguishable in `command_result`. Safe metadata includes nullable exit code, capture
counts and truncation, start/finish Unix milliseconds and the enclosing actual execution ID. Graph results exclude
command text, stdout/stderr, cwd and credentials. Captured command output is never automatically forwarded to a handler,
model or tab. Stop, expiry, Team cleanup and graceful daemon shutdown request cancellation through the exact active
command's native event and its owned cleanup. The shutdown subscription closes admission and wakes waits; it does not
certify that the daemon waited for process settlement before exiting. Cancellation cannot undo external effects. On
Unix, direct commands require macOS process kqueue events or Linux pidfd support; unsupported platforms/kernels refuse
before command spawn. Ordinary observation probes retain their existing wait path. Confirmed command errors allow
independent sibling handlers; unconfirmed resource cleanup stops the source and fences the remaining batch.
Peer-qualified handlers and remote Sentinel execution are unavailable; a graph belongs to one actual connected instance.
Mesh transport does not provide a remote execution claimant or replay contract.

Team Mission Control and the workspace automation entry list named Sentinels; an entry opens its graph. Nodes show
declared purpose, exact scope, state, expiry, handler results and connected targets; cron-to-Worker edges need retained
run and tab IDs. Probe text, task bodies, credentials and secrets are excluded, truncation at the read limit is reported,
and graph Stop validates the exact node scope through authenticated host control.

## Inspect and stop

```bash
codeterm sentinel list --json
codeterm sentinel show SENTINEL_ID --json
codeterm sentinel stop SENTINEL_ID --json
```

These commands show or manage only the calling tab's sentinels in its current Team and workspace. `show` includes the
probe, accepted bucket, hash, next run, expiry, and last decision reason. Stopped watches remain inspectable. Closing a
Team stops and removes all its sentinels, including in-flight route claims. A removed creator or changed Team stops the
watch. The store admits at most 64 active and 256 total sentinels; dormant stages count toward the total. Watches are
local to the creator's machine; `--mesh` is refused.

Stored single-route definitions from older builds and unreadable/corrupt files leave the Sentinel subsystem unavailable
while other subsystems start. Original bytes stay in place; reads and mutations return an actionable load/format error,
never an empty healthy graph. Preserve those definitions and recreate them through current authoring; there is no
automatic migration, reset or discard. Use fresh isolated dev data for demonstrations.

A failed persistence write returns an error and holds Sentinel unavailable; cancellation stays in memory and durability
is unconfirmed until a valid store reload. Repair the filesystem first. No stop success or handler replay is inferred.

<!-- revision: 13 -->
