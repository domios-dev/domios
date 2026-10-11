# Blueprints

A Blueprint watches status with bounded probes at a declared cadence, classifies changed output, or carries a terminal conversation. Use it to wait for CI or builds without repeated agent checks.

Name graphs, handlers and crons by what they watch and do. Create requires `--name`; updates require `name`. Validate with `--dry-run`. Ready skeletons: `codeterm docs BLUEPRINT-RECIPES` or `codeterm blueprint recipe [<name>]`.

## Share a Blueprint

```bash
codeterm blueprint export <id> --out watch.blueprint.json
codeterm blueprint import watch.blueprint.json --set binding_1=<value> --set timeout=1h --stopped
```

Export defaults to stdout. A `.blueprint.json` recipe contains `schema_version: 1`, described `placeholders`, and a `definition` holding a create or pipeline spec. Bind `{{key}}` with `--set`; `create --recipe` also accepts exports. Invalid versions, missing bindings and invalid definitions are refused. Imports use the caller's Team and start stopped; `--start` runs them.

Exports omit owner ids, runtime and history. Commands, input, event parameters, questions, patterns and local targets become placeholders to protect credentials and local references; rebind stage `timeout`. Jobs offers **+ Import Blueprint** with a destination Team tab and bindings, and header **Export Blueprint** downloads.

## When to use a Blueprint graph

Define a graph before repeating an external, agent or cron status check. Unchanged output costs no model call or agent turn; declared buckets route back to you. Observation and actions are deterministic. The model picks a finite bucket, never a command, target or reply. Actions stay in the creator's Team, workspace and machine; `other` means none fits.

Print a stable verdict before long output; only 32 KiB is kept. Timestamps and durations trigger classification, and errors must not resemble success. `--match` classifies first; the Choice adapter handles unmatched output. `--question` defines each bucket and `other`. After creating, end your turn and let a route wake you: waits, sleeps and background loops are polling. Observe same-Team, same-workspace tabs; utility shells need `--terminal-pattern` or `--output-settled`.

Probes receive the creating instance's CLI directory first on `PATH`, API URL/token and socket path. Flapping stops at the transition budget and notifies the creator; hung probes skip ticks without slowing other graphs. Removing an observed tab stops its graph and notifies the creator. Removing a CronRun tab ends that run, notifies the creator and waits for another verified run. A solo creator's graph remains visible after joining a Team.

## Create a watch

```bash
codeterm blueprint create --name ci-finished-watch --probe "gh run view 456 --json status,conclusion" --question "waiting while CI is in progress; finished when CI has completed with any conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h
```

The first probe runs immediately. `--every` defaults to `20s` and must be at least `10s`. Durations accept `s`, `m`, `h`, `d`, and combinations such as `1h30m`; a bare number means minutes. The maximum duration is 366 days. `--timeout` bounds the lifetime. `--until` names one declared bucket and ends the watch after that bucket's first routed transition. `--route bucket=stop` ends it on its first accepted reading.

Every declared bucket routes on its first accepted reading. `--route bucket=stop` ends the graph the first time that bucket is seen; never put `stop` on a still-waiting bucket. Keep a bucket silent with an empty handler list (`{"waiting":[]}`) or `--route waiting=none`. Without rearm-on-change, a route runs again when its bucket changes away and later returns.

For Team watches, a bucket routes only when it changes. Watch each member with its own `--observe tab:<id>` graph, or use `--rearm-on-change <bucket>` when a relevant source change should start a fresh episode.

The creator's live tab supplies its cwd, workspace, and Team. Commands are executed as the daemon user in that cwd, with no interactive stdin, through `/bin/sh -c` on Unix and Windows PowerShell on Windows, where `curl` is an alias (use `curl.exe`) and `>` writes UTF-16. Each probe has a three-second runtime limit (ten seconds on Windows, where PowerShell starts slowly), or the remaining lifetime if shorter. Capture retains up to 32 KiB each of stdout and stderr. The exit status is part of the hashed result. A probe timeout or spawn failure records a skip.

Use `--observe` to watch an agent tab or a cron run instead of running a shell probe:

```bash
codeterm blueprint create --name nightly-run-watch --observe cron-run:nightly --question "identify the current run state" --buckets waiting,finished --every 20s --timeout 1h
```

For `tab:<id>`, the observed tab must share the creator's Team and workspace. Without terminal options it observes agent status and prompts. A `cron-run:<id-or-name>` stage waits for a run, then observes the exact automation tab recorded for that run, even when that hidden tab has no Team membership. A finished run or removed tab unbinds the stage; a replacement run starts a fresh observation episode by resetting the observation hash, accepted bucket, held state, and per-run transition allowance while retaining history. A removed tab routes a notice to the creator agent with the reason.

## Start from data: input stages

An input stage watches data instead of a probe or observation: inline text, a file, or a URL. `--input <text|audio|video|image>` takes the media kind; provide exactly one source with `--input-file <PATH>` (absolute or cwd-relative), `--input-url <URL>` (http/https), or `--input-text <TEXT>` (text kind only, ≤64 KiB, no NUL). `--convert-to <text|audio>` requests a one-time conversion to a different kind; `--convert-timeout <DURATION>` bounds it (default 15 min, max 60 min). All input flags conflict with `--probe`, `--observe`, `--every` and `--rearm-on-change`. The media kind is immutable under `codeterm blueprint update`; `data` and `convert_to` are amendable. The graph `source` field shows kind, ref type and display name — never inline text or URL queries. Text input stages deliver their initial source once, then wait for explicit text. Use `codeterm blueprint input <id> --from-pad <relative-path>` or `--text-file <path>` to enqueue another UTF-8 body (1–32768 bytes, no NUL). Each message is classified and routed, including repeated identical messages. Admission returns `accepted`, not a delivery receipt. The in-memory FIFO holds at most 32 messages per stage; full, stopped, paused, expired and foreign stages refuse input. Queued bodies are discarded on definition changes, stop or daemon restart. Other media and text conversion still record `input_runtime_unavailable`.

## Classification and transitions

A connected Supporter adapter with Choice capability classifies output that matches no deterministic pattern, using one question over `--buckets` and the automatic `other` option with the shared request budget and journal. Probe output is treated as untrusted data. Unchanged output never calls the model, including after a failed or budget-exhausted classification. Deterministic patterns are checked first, even when a Choice adapter is connected. One matching bucket is accepted at confidence 1; overlapping matches become `other`. Unmatched output uses the adapter when connected, or becomes `other` when no adapter is connected:

```bash
codeterm blueprint create --name ci-result-watch --probe "check-ci" --question "CI result" --buckets waiting,passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --every 20s --until passed --timeout 1h
```

Patterns see the formatted result: exit status, stdout, then stderr. Match the status line with a multiline pattern such as `(?m)^success$` when the probe prints `success`. Create without either a connected Choice adapter or a pattern is refused.

Routes run once when a new accepted bucket differs from the previous accepted bucket, including the first accepted result. Model confidence must be at least 0.80. `other` and low confidence can use the reserved `other` handlers; adapter failure and exhausted budgets never escalate. Unknown or weak observations do not reset the last accepted bucket. Returning to a previously accepted bucket after another accepted bucket is a new transition.

Use `--rearm-on-change <bucket>` on create, or a `rearm_on_change` array in update JSON, to route again when the output hash changes but the confident result stays in that bucket. Each rearm is an accepted transition and consumes the configured budget.

## Closed-set routes

Repeat `--route bucket=action` for inline `notify`, `blueprint`, `send`, `cron`, or `stop` handlers. Use `--handlers-file` for `command`, `respond_prompt`, or `escalate` handlers and when explicit handler IDs are needed. Unassigned buckets use `notify`.

| Action | Result |
|---|---|
| `notify` | Sends the result to the creator agent; a shell creator gets a notification record |
| `blueprint:<exact-id>` | Activates an existing dormant stage owned by the same creator, Team and workspace |
| `send:<tab>` | Delivers the stage text body: typed input plus Enter to a shell, routed message to an agent |
| `cron:<name>` | Triggers the uniquely named existing cron |
| `respond_prompt:<kind>` | Requests an answer through the existing provider-declared safe-response path |
| `stop` | Stops the graph on this first accepted reading |
| `none` | Declares the bucket with no handlers |
| `escalate:creator` or `escalate:agent:<tab>` | Sends a small escalation packet through the message gateway |

A send target must share the creator's Team and workspace. The existing safe send path delivers the body without shell interpolation; agent routing retains its gateway/AgentPad handling. Cron targets must belong to the same Team and workspace. Targets resolve at creation and are checked again at routing. No route executes a command chosen by the model. Each accepted transition and handler attempt has a durable identity. A claim is saved before its action; claimed handlers are never replayed after restart. Independent successes, failures, skips and unconfirmed outcomes stay visible. Completion confirms the selected operation, not recipient work or cron success. A failed handler does not erase earlier results or prevent later handlers; stop cancels the remaining handlers. A missing result after a crash is unconfirmed.

`respond_prompt:<kind>` requires an `--observe` tab or cron-run source. The kind identifies a requested safe-response path; it does not authorize an answer or select an option. For example, `permission` can request a response to a per-tool permission prompt. The existing responder revalidates the exact current prompt and kind, then chooses only a provider-declared option through the existing safe-response writer. Changed prompts, unsupported kinds, unsafe targets, and prompts without a matching declared option are refused. Blueprint does not write raw PTY keys or construct provider steps. Each tab accepts at most one response per prompt occurrence, and Blueprint prompt-response requests are limited to 600 per rolling 60-second window.

## Compose a named Blueprint graph

Create downstream stages first with `--paused`, then create the named entry whose outputs activate their exact IDs. Paused stages do not probe or spend Choice requests. Their timeout starts at creation and continues while dormant.

```bash
codeterm blueprint create --name "Review stage" --paused --probe "check-review" --question "done when review completes" --buckets done --match 'done=(?m)^done$' --until done --timeout 1h --json
codeterm blueprint create --name "CI and review" --probe "check-ci" --question "passed or failed CI" --buckets passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --route passed=blueprint:sentinel_DOWNSTREAM_ID --route passed=notify --route failed=stop --timeout 1h
```

Replace `sentinel_DOWNSTREAM_ID` with the exact ID from the first response. The entry and its reachable stages appear as one named scenario. Shared targets retain their identity and results. Activating an already active stage is a noop: it does not reset its accepted result, schedule, expiry or running work. Missing, foreign, self, stopped and expired targets are refused. Rearming is limited to `--rearm-on-change`; there is no recursive scheduler execution or cyclic-episode guarantee.

Use `--handlers-file PATH` for JSON bucket-to-array definitions. Omit `id` to generate a meaningful handler-node name from its type and bucket (for example, `send-to-review-on-passed`). Explicit generic IDs such as `default_1` or `handler_2` are refused; replace them with a purpose-based name or omit them:

```json
{"passed":[{"id":"review","type":"blueprint","parameters":{"target":"sentinel_DOWNSTREAM_ID"}},{"id":"notice","type":"notify","parameters":{}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```

The file form cannot be mixed with `--route`. Unknown fields or types, duplicate IDs and ambiguous targets are refused. At most 16 handlers fit each bucket and 64 fit one node. Composition definitions require `--timeout`. Each graph has a transition budget that defaults to 64 and can be set up to 256 with `--max-transitions`. Probe and direct-tab graphs spend it over their lifetime; each verified CronRun binding gets a fresh allowance. Accepted history is retained to a separate lifetime cap of 65,536 transitions. Exhaustion stops the graph with `transition_budget_exhausted` and routes a notice with the reason to the creator agent. The action-attempt budget defaults to 256 and `--max-actions` allows up to 1024. No model output constructs a handler, target or command. A fixed local command is available only through `--handlers-file`, with creator-authored parameters:

```json
{"passed":[{"id":"marker","type":"command","parameters":{"command":"printf marker > marker.txt","label":"Write marker","timeout_ms":1000}},{"id":"review","type":"blueprint","parameters":{"target":"sentinel_DOWNSTREAM_ID"}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```

Use the same file form to declare a prompt answer:

```json
{"ready":[{"id":"answer_permission","type":"respond_prompt","parameters":{"target":"permission"}}]}
```

Save the JSON as `handlers.json` in your disposable fixture directory, replace the downstream ID, and create from that directory:

```bash
codeterm blueprint create --name "Marker and review" --probe "printf passed" --question "passed or failed" --buckets passed,failed --match 'passed=(?m)^passed$' --match 'failed=(?m)^failed$' --handlers-file handlers.json --timeout 1h
```

Put escalation actions only in the reserved `other` array, for example:

```json
{"other":[{"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}}]}
```

It accepts only `notify` and `escalate` handlers, with at most one escalation. An escalation packet carries only Blueprint and transition IDs, never probe output. Creating a graph with `other` handlers requires positive `--escalation-timeout-ms`; updates must set a positive `escalation_timeout_ms` in the definition JSON. After an escalation the stage holds without classifying until resolution, a new run binding, or timeout (journaled as `escalation_expired`). Resolve an open episode with:

```bash
codeterm blueprint escalation resolve BLUEPRINT_ID TRANSITION_ID
```

Resolution clears the observation hash and re-evaluates once. An escalation claim is persisted before delivery and is never replayed after restart. The same cause escalates at most once per five minutes per Blueprint.

Update a definition from JSON with the current version. Definitions require a meaningful `name`. Add `--cause TRANSITION_ID` when the amendment addresses an escalation. Any caller in the same Team and workspace may amend it; each accepted update records version lineage:

```bash
codeterm blueprint update BLUEPRINT_ID --expect-version VERSION --from-file definition.json
codeterm blueprint update BLUEPRINT_ID --expect-version VERSION --from-file definition.json --cause TRANSITION_ID
codeterm blueprint history BLUEPRINT_ID
```

For an escalation, inspect the definition and versions with `codeterm blueprint history <id>`, update it with the transition as `--cause`, then resolve with `codeterm blueprint escalation resolve <id> <transition>`.

Command text is immutable, nonempty, at most 8192 bytes and contains no NUL. Its display label is at most 256 bytes. `timeout_ms` is positive and no larger than the platform probe limit. Execution uses the shorter of that duration, the remaining node lifetime and the platform cap, reserving cleanup once. The authenticated creator cwd supplies the working directory; there is no handler cwd or environment option. The shell interprets ordinary syntax that the creator wrote. The host never substitutes observation/model output into commands. Process containment is not a filesystem sandbox: commands can access files available to the daemon account.

Each command claim is persisted before launch and is never replayed, including an uncertain crash. Success requires actual zero exit and confirmed parent, boundary and capture settlement. Nonzero, API/spawn error, cancellation, deadline and unconfirmed resources remain distinguishable in `command_result`. Safe metadata includes nullable exit code, capture counts and truncation, start/finish Unix milliseconds and the enclosing actual execution ID. Graph execution results exclude command text, stdout/stderr, cwd and credentials. Captured command output is never automatically forwarded to a handler, model or tab. Stop, expiry, Team cleanup and graceful daemon shutdown request cancellation through the exact active command's native event and its owned cleanup. The shutdown subscription closes admission and wakes waits; it does not certify that the daemon waited for process settlement before exiting. Cancellation cannot undo external effects. On Unix, direct commands require macOS process kqueue events or Linux pidfd support; unsupported platforms/kernels refuse before command spawn. Ordinary observation probes retain their existing wait path. Confirmed command errors allow independent sibling handlers; unconfirmed command resource cleanup stops the source and fences the remaining batch. Read-only probe capture or completion failures skip the tick only when the child and its process boundary are positively confirmed exited (including killed and reaped children). Capture-only uncertainty remains journaled; unconfirmed child or process-boundary resources stop the source. Three consecutive capture failures stop the source; a successful tick or another failure kind resets the count. The next scheduled tick is the only retry. Peer-qualified handlers and remote Blueprint execution are unavailable; a graph belongs to one actual connected instance. Mesh transport does not provide a remote execution claimant or replay contract.

Team Mission Control and the workspace automation entry list named Blueprints; an entry opens its graph. Nodes show declared purpose, exact scope, state, expiry, handler results and connected targets; cron-to-Worker edges need retained run and tab IDs. Task bodies, raw stdout/stderr, cwd, environment and credentials are excluded, truncation at the read limit is reported, and graph Stop validates the exact node scope through authenticated host control.

An exact Team graph view may include version-1 node detail for that Team's stored definition: redacted probe text, question, bucket patterns and command handlers. This narrow exception does not expose execution output or environment. The workspace overview returns `definition.unavailable`; a mismatched scope never grants definition access. Probe and command text are capped at 4096 bytes with explicit truncation flags; an unrepresentable definition or one exceeding 64 KiB is unavailable. Pipeline definitions are unavailable in version 1. Origin remains unknown without positive provenance. An absent creator name does not imply system ownership. Accepted transitions retain pattern or adapter provenance, adapter/model identity and confidence separately from the configured fallback. `adapter_used` stays null when retained history lacks decision provenance. A next-check timestamp appears only for an authoritative active schedule.

## Inspect, stop and delete

```bash
codeterm blueprint list --json
codeterm blueprint show BLUEPRINT_ID --json
codeterm blueprint stop BLUEPRINT_ID --json
codeterm blueprint delete BLUEPRINT_ID [--stop]
codeterm blueprint delete --stopped
```

`delete` removes a whole Blueprint with every stage and its history; naming a stage deletes the Blueprint it belongs to. Only the creator tab or the Team owner may delete, and only inside the caller's Team and workspace. A running Blueprint is refused unless `--stop` is passed. `--stopped` deletes every stopped Blueprint the caller may delete and prints the removed ids; deletion cannot be undone.

These commands show or manage only the calling tab's blueprints in its creation scope and workspace. A solo graph remains visible to its creator after that tab joins a Team; it is not shared with the new Team. `show` includes the probe, accepted bucket, hash, next run, expiry, and last decision reason. Stopped watches remain inspectable. Closing a Team stops and removes its Team-owned blueprints, including in-flight route claims. A removed creator stops the watch. The store admits at most 64 active and 256 total blueprints; dormant stages count toward the total. Watches are local to the creator's machine; `--mesh` is refused.

Legacy single-route definitions or corrupt stores make Blueprints unavailable; other subsystems still start. Original bytes remain, and reads and mutations return a load/format error. Preserve the definitions and recreate them through current authoring: no automatic migration, reset or discard. Demonstrate with fresh isolated dev data.

Failed persistence holds Blueprints unavailable until filesystem repair and a valid reload. Cancellation remains in memory; durability, stop success and replay are unconfirmed.



## Terminal output and conversation mode

Use `--observe tab:<id>` with `--terminal-pattern <regex>`, `--output-settled <milliseconds>`, or both. Terminal observations subscribe to accepted PTY output; they never run a shell probe or poll `tab get-text`. Pattern matching runs against the new output delta. Output-settled fires after 10–60000 ms without a semantic output change; it reports output settling, not harness readiness or successful submission. ANSI sequences are removed across chunk boundaries, carriage-return redraws replace the current line, and braille spinners and duration counters are normalized. Each event carries at most 32 KiB of UTF-8 text. Captured output is untrusted data.

`send:<tab>` forwards that text body unchanged. A utility shell receives typed input plus Enter through the safe PTY send path; an agent receives a routed message through the gateway. The host never interpolates captured data into a shell command. Sending to a shell can execute what its foreground program interprets as a command, so target the harness tab deliberately. `notify` retains the creator notification/result-file behavior. A full terminal event queue stops its source with `terminal_queue_full`; replacing the PTY or definition discards obsolete deltas.

`--session` makes message exchanges independent of `--max-transitions` and `--max-actions`. Session stages share a safety allowance within their graph: 16384 accepted exchanges or action attempts, or 120 accepted exchanges within a rolling minute. Safety counts remain after reload; conversation counts and limits appear in Runs. Ordinary finite watches retain their configured budgets. Use `--timeout` to bound a conversation's lifetime and `codeterm blueprint stop <id>` to end it explicitly.

## Pipelines

Pipelines combine a schedule/input/event trigger, named command/convert/ask/filter steps, outcome routes and a result target. Jobs shows real steps and retained Runs, including shell-created Blueprints. Closed runs have no current step. Pattern decisions read Pattern rules. Logs include names and signed indices (`-1` for `$run`/`$result`).

```bash
codeterm blueprint create --recipe echo --set message=ready
codeterm blueprint log BLUEPRINT_ID
```

## Bounded Agent steps

A typed pipeline may include `{"kind":"agent","provider":"<capable-provider>","profile":"<profile>","task":"<instruction>","deadline_ms":300000,"max_tokens":64000,"max_turns":24}` as a step's `kind`. Input and output are text. Discover providers and their profiles through `codeterm blueprint nodes`; the provider must declare bounded Agent tasks. Rotterdom implements this capability through its configured profiles. An unsupported provider is refused before creation and checked again before execution.

The task and preceding output reach the provider unchanged, with token, turn and deadline ceilings. The provider returns structured JSON `{status, output, error?}`, with status `done`, `failed`, `cancelled` or `deadline_exceeded`. The step routes `done` or `failed`. A graph stop aborts the active task; its step log closes as failed. Results, including abort status, persist in `blueprint-agent-results/` and the step log exposes their `body_ref`. Claimed steps never replay after restart, and an unfinished tool effect remains unconfirmed. Agent steps do not turn model output into a shell command.

<!-- revision: 27 -->
