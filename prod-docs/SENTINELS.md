# Sentinels

A sentinel watches external status cheaply while an agent does useful work. It runs a bounded shell probe at the
requested cadence, hashes the captured result, and classifies only changed output. Use it to wait for CI or build
completion instead of repeatedly asking an agent to run the same status check.

## When to use a Sentinel graph

Before you repeat a check or wait on an external, agent or cron status, describe the loop once as a graph instead of spending turns on it. Unchanged output costs no model call and no agent turn; you wake only when a declared bucket routes to you. Observation and actions are deterministic, the model only picks one of your finite buckets (never a command, target or reply), and every action stays in the creator's Team, workspace and machine. `other` means no bucket fits.

Make the probe print one stable verdict: no timestamps or durations (every changed byte costs a classifier call), the verdict before any long output (only the first 32 KiB is kept), and errors that cannot pass for a result. Deterministic `--match` patterns classify matching output first; a connected Choice adapter classifies output that matches no pattern. The `--question` names every bucket and what is `other`. After creating a graph, end your turn: a route wakes you, while a wait tool, `sleep` or background shell loop is still polling. Only agent tabs can be observed; a utility shell writes a status file instead.

1. **CI or PR result.** `--probe "gh run view ID -R OWNER/REPO --json status,conclusion"`, buckets `passed,failed`, the question naming running CI and probe errors as `other` (without `-R` outside a checkout, gh's error can be read as `failed`); `--route passed=cron:<review>` and `failed=send:<fixer tab>`. The branch is decided once, no polling.
2. **A Worker waiting on a prompt.** `--observe tab:<worker>`, buckets `prompt,busy` in `--handlers-file`: `prompt` to `send:<owner tab>` or `notify`, `"busy": []` for working or idle. Do not escalate `other` here: the hold would hide the next prompt until resolved. The tab is read, never typed into.
3. **An agent finished its turn.** Instead of re-reading `tab get-text`: `--observe tab:<agent>`, buckets `working,idle,prompt`, `"working": []`, `idle` and `prompt` to `notify`, and `--until idle`.
4. **Deploy or service health.** `--probe "curl -fsS --max-time 2 https://HOST/health"` (`curl.exe` on Windows), buckets `healthy,down`, `down=notify`. Only a change of bucket routes; an unassigned `healthy` uses `notify`, so you also wake on the first read and on recovery (`"healthy": []` keeps it silent). To wait for a server to come up, declare only `ready` with `--until ready`: refusals stay `other`.
5. **Long build, test or QA batch.** Have the job write `passed` or `failed` to a status file; `--probe "cat out/test.status"`. While the file is missing the result is a quiet `other`; the default `notify` wakes the creator on each result instead of re-reading a log each turn.
6. **Threshold.** Compute the verdict inside the probe and print one word, e.g. in PowerShell `if ((Get-PSDrive C).Free -lt 20GB) {"low"} else {"ok"}`, with `"ok": []` and `low` to `notify`: you wake when the value crosses, not on every reading.
7. **Self-improving loop.** Escalate `other` to the owning agent (`escalate:agent:<tab>` in `--handlers-file`). It reads `sentinel history`, adds the missing bucket or handler with `sentinel update ... --cause TRANSITION_ID`, then resolves the escalation so the stage re-evaluates once. Each unknown case costs one turn, then stays a versioned branch. The update file is not the `history` definition: it takes `question`, `buckets`, `every`, `handlers` (the create file form), `on_other`, `escalation_timeout_ms`, `max_transitions` and `max_actions`; the probe or observed source cannot change.
8. **Watching a whole Team.** A General Agent probes `codeterm tab status --tab <orchestrator-tab>` (it reports `not found` once the tab is gone; `orchestrator status` does not) and `codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>` filtered to its `live_lanes` and `drift` lines (the reminder line changes every tick). Buckets `healthy,stuck,needs_owner,lost` in `--handlers-file`: `"healthy": []`; `stuck`, `needs_owner` and `lost` each `notify` the creator once per change; leave `other` quiet, since an escalation hold would hide later changes. A `send` target must share the creator's Team, so the General Agent relays `stuck` to the Team Orchestrator with `codeterm send` and takes `needs_owner` to the owner.

A probe puts the creating instance's installed CLI directory first on `PATH` and receives that instance's API URL, configured token and socket path, so `codeterm` commands use the right daemon. A flapping source routes until its transition budget is exhausted; the graph stops and notifies its creator with the reason. A hung probe skips each tick without slowing other graphs. Removing a directly observed tab stops that graph and notifies its creator. Removing a CronRun tab ends that run's observation, notifies the creator, and leaves the graph waiting for another verified run. A graph created while its creator is solo remains visible to that creator after they join a Team.

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

The creator's live tab supplies its cwd, workspace, and Team. Commands are executed as the daemon user in that cwd, with no interactive stdin, through `/bin/sh -c` on Unix and Windows PowerShell on Windows, where `curl` is an alias (use `curl.exe`) and `>` writes UTF-16. Each probe has a three-second runtime limit (ten seconds on Windows, where PowerShell starts slowly), or the remaining lifetime if shorter. Capture retains up to 32 KiB each of stdout and stderr. The exit status is part of the hashed result. A probe timeout or spawn failure records a skip.

Use `--observe` to watch an agent tab or a cron run instead of running a shell probe:

```bash
codeterm sentinel create --observe cron-run:nightly --question "identify the current run state" --buckets waiting,finished --every 20s --timeout 1h
```

For `tab:<id>`, the observed tab must be an agent in the creator's Team and workspace. A `cron-run:<id-or-name>` stage
waits for a run, then observes the exact automation tab recorded for that run, even when that hidden tab has no Team
membership. A finished run or removed tab unbinds the stage; a replacement run starts a fresh observation episode by
resetting the observation hash, accepted bucket, held state, and per-run transition allowance while retaining history.
A removed tab notifies the creator with the reason.

## Classification and transitions

A connected Supporter adapter with Choice capability classifies output that matches no deterministic pattern, using
one question over `--buckets` and the automatic `other` option with the shared request budget and journal. Probe
output is treated as untrusted data. Unchanged output never calls the model, including after a failed or
budget-exhausted classification.

Deterministic patterns are checked first, even when a Choice adapter is connected. One matching bucket is accepted at
confidence 1; overlapping matches become `other`. Unmatched output uses the adapter when connected, or becomes
`other` when no adapter is connected:

```bash
codeterm sentinel create --probe "check-ci" --question "CI result" --buckets waiting,passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --every 20s --until passed --timeout 1h
```

Patterns see the formatted result: exit status, stdout, then stderr. Match the status line with a multiline pattern such
as `(?m)^success$` when the probe prints `success`. Create without either a connected Choice adapter or a pattern is
refused.

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
a transition budget that defaults to 64 and can be set up to 256 with `--max-transitions`. Probe and direct-tab graphs
spend it over their lifetime; each verified CronRun binding gets a fresh allowance. Accepted history is retained to a
separate lifetime cap of 65,536 transitions. Exhaustion stops the graph with `transition_budget_exhausted` and notifies
the creator once with the reason. The action-attempt budget
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

These commands show or manage only the calling tab's sentinels in its creation scope and workspace. A solo graph remains
visible to its creator after that tab joins a Team; it is not shared with the new Team. `show` includes the probe,
accepted bucket, hash, next run, expiry, and last decision reason. Stopped watches remain inspectable. Closing a Team
stops and removes its Team-owned sentinels, including in-flight route claims. A removed creator stops the watch. The
store admits at most 64 active and 256 total sentinels; dormant stages count toward the total. Watches are
local to the creator's machine; `--mesh` is refused.

Stored single-route definitions from older builds and unreadable/corrupt files leave the Sentinel subsystem unavailable
while other subsystems start. Original bytes stay in place; reads and mutations return an actionable load/format error,
never an empty healthy graph. Preserve those definitions and recreate them through current authoring; there is no
automatic migration, reset or discard. Use fresh isolated dev data for demonstrations.

A failed persistence write returns an error and holds Sentinel unavailable; cancellation stays in memory and durability
is unconfirmed until a valid store reload. Repair the filesystem first. No stop success or handler replay is inferred.

<!-- revision: 15 -->
