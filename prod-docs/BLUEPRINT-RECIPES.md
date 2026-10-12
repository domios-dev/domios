# Blueprint recipes

Ready Blueprint graph patterns and skeletons. Sources, classification, routes, handlers, escalation, updates and limits are in `codeterm docs BLUEPRINTS`. Name each graph by meaning: what is watched → what happens.

## Read a graph as Trigger → Source → Agent or Logic → Action
The Blueprint view draws every stage as four kinds of node, so write definitions that read well in that order:

- **Trigger** starts the stage: a schedule (`--every`), an event from an observed tab or terminal, text input (`blueprint input`), or activation by an earlier stage (`blueprint:<id>` on that stage, with this one `--paused`).
- **Source** collects what the decision receives: a probe command (`--probe`, the only node that shows code besides command actions), a Team terminal (`--observe tab:<id>` with `--terminal-pattern` or `--output-settled`), an observed tab or cron run, or input data.
- **Agent** decides with `--question`: write it as the instruction the agent follows, naming every bucket and what it means (`passed: CI finished green; failed: a job failed`). Each bucket becomes an exec pin labelled with that definition, so keep one clause per bucket. **Logic** is the deterministic form: `--match bucket=regex` patterns, checked before the agent.
- **Action** runs per bucket: `notify`, `send:<tab>`, `cron:<name>`, a labelled `command`, `blueprint:<id>` for the next stage, `escalate`, `respond_prompt` or `stop`. A bucket with `[]` is an open pin: nothing happens. Every action receives the source output as its payload.

Name the graph for what the whole chain does, and give each command a short label; the label is the action's title. `codeterm blueprint nodes` lists every node type in this vocabulary.

## Use-case patterns
Each pattern names the source, buckets and handlers; put silent buckets as `[]` and check the result with `--dry-run`.

1. **CI or PR result.** `--probe "gh run view ID -R OWNER/REPO --json status,conclusion"`, buckets `passed,failed`, the question naming running CI and probe errors as `other` (without `-R` outside a checkout, gh's error can be read as `failed`); `--route passed=cron:<review>` and `failed=send:<fixer tab>`. The branch is decided once, no polling.
2. **A Worker waiting on a prompt.** `--observe tab:<worker>`, buckets `prompt,busy` in `--handlers-file`: `prompt` to `send:<owner tab>` or `notify`, `"busy": []` for working or idle. Do not escalate `other` here: the hold would hide the next prompt until resolved. The tab is read, never typed into.
3. **An agent finished its turn.** Instead of re-reading `tab get-text`: `--observe tab:<agent>`, buckets `working,idle,prompt`, `"working": []`, `idle` and `prompt` to `notify`, and `--until idle`.
4. **Deploy or service health.** `--probe "curl -fsS --max-time 2 https://HOST/health"` (`curl.exe` on Windows), buckets `healthy,down`, `down=notify`. Only a change of bucket routes; an unassigned `healthy` uses `notify`, so you also wake on the first read and on recovery (`"healthy": []` keeps it silent). To wait for a server to come up, declare only `ready` with `--until ready`: refusals stay `other`.
5. **Long build, test or QA batch.** Have the job write `passed` or `failed` to a status file; `--probe "cat out/test.status"`. While the file is missing the result is a quiet `other`; the default `notify` wakes the creator on each result instead of re-reading a log each turn.
6. **Threshold.** Compute the verdict inside the probe and print one word, e.g. in PowerShell `if ((Get-PSDrive C).Free -lt 20GB) {"low"} else {"ok"}`, with `"ok": []` and `low` to `notify`: you wake when the value crosses, not on every reading.
7. **Self-improving loop.** Escalate `other` to the owning agent (`escalate:agent:<tab>` in `--handlers-file`). It reads `codeterm blueprint history <id>`, adds the missing bucket or handler with `codeterm blueprint update ... --cause <transition>`, then resolves with `codeterm blueprint escalation resolve <id> <transition>` so the stage re-evaluates once. Each unknown case costs one turn, then stays a versioned branch. The update file is not the `history` definition: it takes `name`, `question`, `buckets`, `every`, `handlers` (the create file form), `on_other`, `rearm_on_change`, `escalation_timeout_ms`, `max_transitions` and `max_actions`; the probe or observed source cannot change.
8. **Watching a whole Team.** A General Agent probes `codeterm tab status --tab <orchestrator-tab>` (it reports `not found` once the tab is gone; `orchestrator status` does not) and `codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>` filtered to its `live_lanes` and `drift` lines (the reminder line changes every tick). Buckets `healthy,stuck,needs_owner,lost` in `--handlers-file`: `"healthy": []`; `stuck`, `needs_owner` and `lost` each `notify` the creator once per change; leave `other` quiet, since an escalation hold would hide later changes. A `send` target must share the creator's Team, so the General Agent relays `stuck` to the Team Orchestrator with `codeterm send` and takes `needs_owner` to the owner.

## Built-in recipes
`codeterm blueprint recipe` lists these skeletons. `codeterm blueprint recipe <name>` prints the handlers JSON and create command; fill in the probe, question and target placeholders, then use `--dry-run` before creating the graph. Creation proves admission only: confirm one real probe run and its recognized bucket with `blueprint show`. Patterns see `exit=N`, stdout, then stderr, so anchor per line with `(?m)` and allow CRLF with `\r?$`.
<!-- blueprint-recipe:github-pr-ci-watch -->
**Purpose:** Route `failed` on the first failed or cancelled check of one PR and `passed` when all passed; `waiting` stays silent. From the repository checkout, the probe prints one verdict line: `gh pr checks <PR> --json bucket -q 'if any(.[]; .bucket=="fail" or .bucket=="cancel") then "failed" elif any(.[]; .bucket=="pending") then "waiting" else "passed" end'`.
```json
{"waiting":[],"passed":[{"type":"notify","parameters":{}}],"failed":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name pr-ci-watch --probe "<PR_VERDICT_PROBE>" --question classify-pr-checks --buckets waiting,passed,failed --match 'waiting=(?m)^waiting\r?$' --match 'passed=(?m)^passed\r?$' --match 'failed=(?m)^failed\r?$' --until passed --rearm-on-change failed --handlers-file handlers.json --every 60s --timeout 3h --dry-run
```
<!-- /blueprint-recipe -->
<!-- blueprint-recipe:team-watch -->
### team-watch
**Purpose:** Observe one Team member's tab and notify when the watched state changes.
```json
{"waiting":[],"changed":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name team-member-watch --observe 'tab:<TAB_ID>' --question classify-member-state --buckets waiting,changed --match 'changed=<CHANGED_PATTERN>' --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:wait-for-build -->
### wait-for-build
**Purpose:** Wait for a build verdict and notify on either terminal result.
```json
{"waiting":[],"passed":[{"type":"notify","parameters":{}}],"failed":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name build-result-watch --probe "<READ_ONLY_BUILD_PROBE>" --question classify-build-result --buckets waiting,passed,failed --match 'passed=<PASSED_PATTERN>' --match 'failed=<FAILED_PATTERN>' --until passed --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:retry-until-pass -->
### retry-until-pass
**Purpose:** Keep watching a bounded retry process until its pass result arrives.
```json
{"waiting":[],"passed":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name retry-pass-watch --probe "<RETRY_STATUS_PROBE>" --question classify-retry-result --buckets waiting,passed --match 'passed=<PASSED_PATTERN>' --until passed --timeout 1h --handlers-file handlers.json --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:prompt-responder -->
### prompt-responder
**Purpose:** Request a safe response to a supported permission prompt on one observed agent tab.
```json
{"permission":[{"type":"respond_prompt","parameters":{"target":"permission"}}]}
```
```bash
codeterm blueprint create --name permission-prompt-reply --observe 'tab:<TAB_ID>' --question classify-current-prompt --buckets permission --match 'permission=<PERMISSION_PATTERN>' --until permission --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:cron-run-watch -->
### cron-run-watch
**Purpose:** Observe one named cron's run and notify when it finishes.
```json
{"waiting":[],"finished":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name cron-run-finished-watch --observe 'cron-run:<CRON_NAME_OR_ID>' --question classify-run-state --buckets waiting,finished --match 'finished=<FINISHED_PATTERN>' --until finished --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:launch-prompt-gate -->
### launch-prompt-gate
**Purpose:** Answer each scheduled run's launch prompts through the safe-response path; escalate any other prompt to the creator.
```json
{"clear":[],"trust":[{"type":"respond_prompt","parameters":{"target":"directory_trust"}}],"provider":[{"type":"respond_prompt","parameters":{"target":"onboarding"}},{"type":"respond_prompt","parameters":{"target":"permission"}}],"update":[{"type":"respond_prompt","parameters":{"target":"update"}}],"other":[{"type":"escalate","parameters":{"target":"creator"}}]}
```
```bash
codeterm blueprint create --name launch-prompt-gate --observe 'cron-run:<CRON_NAME_OR_ID>' --question classify-launch-prompt --buckets clear,trust,provider,update --match 'clear=\x22prompt\x22:null' --match 'trust=\x22kind\x22:\x22directory_trust\x22' --match 'provider=\x22kind\x22:\x22provider\x22' --match 'update=\x22kind\x22:\x22manual_update\x22' --rearm-on-change trust --rearm-on-change provider --rearm-on-change update --handlers-file handlers.json --escalation-timeout-ms 1800000 --timeout 30d --dry-run
```
<!-- /blueprint-recipe -->

Patterns match the observed prompt kind (`\x22` is a shell-safe regex quote); rearm answers each new prompt. Onboarding and permission share `provider`, so the handler not matching the live screen records `sentinel_prompt_target_refused`. Owned launch tabs answer trust and onboarding without a graph through the same writer, one answer per prompt; the graph adds a journaled branch per prompt and escalates the rest.

## Pipeline authoring
Create a pipeline from one JSON or YAML file, or start with a named recipe and replace its declared placeholders. Use `--dry-run` to see the normalized chain and each validation check before creating it. Use `codeterm blueprint nodes [--json]` to inspect the available trigger, step, converter and event-source capabilities; use `codeterm blueprint log <ID> [--limit N] [--json]` to inspect bounded per-step run history.

<!-- blueprint-pipeline-recipe:echo -->
### echo
**Purpose:** Run one fixed command, match its output, and return the result to the owner tab.
```yaml
name: Echo pipeline
owner: tab
trigger:
  type: schedule
  every: 20s
steps:
  - name: emit
    input: text
    kind:
      kind: command
      command: "printf '{{message}}'"
  - name: match
    input: text
    kind:
      kind: filter
      pattern: "^{{message}}$"
      bucket: matched
    on:
      matched: stop
result:
  to: owner
```
```bash
codeterm blueprint create --recipe echo --set message=ready --dry-run
```
<!-- /blueprint-pipeline-recipe -->

## Update a definition from history
Save the JSON history response unchanged and pass it back as the update file with the current expected version:
```bash
codeterm blueprint history <ID> --json > history.json
codeterm blueprint update <ID> --expect-version <VERSION> --from-file history.json
```
<!-- blueprint-recipe:all-finished -->
### Several agents or jobs all finished
**Purpose:** Use one verdict probe for all work; keep `waiting` silent and route only `alldone`.
```json
{"waiting":[],"alldone":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name agents-all-finished-watch --probe "<ONE_VERDICT_PROBE>" --question classify-all-work --buckets waiting,alldone --match 'alldone=<ALL_TERMINAL_PATTERN>' --until alldone --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /blueprint-recipe -->

<!-- blueprint-recipe:voice-note-to-text -->
### voice-note-to-text
**Purpose:** Convert a voice note to text and classify the transcript.
```json
{"done":[{"type":"notify","parameters":{}}]}
```
```bash
codeterm blueprint create --name voice-note-to-text --input audio --input-file '<PATH_TO_VOICE_NOTE>' --convert-to text --question what-does-the-voice-note-say --buckets done --handlers-file handlers.json --timeout 1h --dry-run --model-classification
```
<!-- /blueprint-recipe -->

## CI result routed to review or repair
For quiet PR waiting, declare only terminal outcomes; classify still-running CI as `other`, and add an `other`
escalation handler when it needs attention. With an existing same-Team cron and agent:
```bash
codeterm blueprint create --name ci-result-review-watch --probe "gh run view 456 --json status,conclusion" --question "passed for completed successful CI; failed for completed unsuccessful CI; other while running" --buckets passed,failed --route passed=cron:review_pr --route failed=send:repair_worker --timeout 1h --model-classification
```

## Terminal adapter
Use this when the harness has no provider plugin. Run the harness as a foreground process in a Team utility tab, then attach an output watcher before sending its first request. Replace the placeholders with the actual command, tab id and graph ids returned by the CLI. The calling agent must belong to the same Team.
```bash
codeterm tab new --team <TEAM_ID> --title Harness --eager --cmd "<HARNESS_COMMAND>"
codeterm blueprint create --name harness-replies --observe tab:<HARNESS_TAB_ID> --output-settled 300 --session --question "reply when the harness produces output" --buckets reply --match 'reply=(?s).+' --route reply=notify --timeout 1h
codeterm blueprint create --name harness-input --input text --input-file <INITIAL_REQUEST_FILE> --session --question "send every input body to the harness" --buckets request --match 'request=(?s).+' --route request=send:<HARNESS_TAB_ID> --timeout 1h
```
The input graph sends its initial request once. Every settled reply wakes the creator with a bounded result file; read that file, decide the next request, write its exact UTF-8 body to `request.txt` in your Agent Pad, and deliver it:
```bash
codeterm blueprint input <INPUT_GRAPH_ID> --from-pad request.txt
codeterm blueprint show <INPUT_GRAPH_ID>
```
For a file outside the pad, use `codeterm blueprint input <INPUT_GRAPH_ID> --text-file <PATH>`. A prompt-shaped harness can use `--terminal-pattern '<PROMPT_REGEX>'` instead of or alongside `--output-settled`. To route replies directly to another Team agent, use `--route reply=send:<AGENT_TAB_ID>` in the watcher. Never feed a watcher back to the same harness without a protocol that distinguishes replies from new requests.

Verify the far end: the harness must report the result of the request, not merely echo its input. For an echo fixture, make it print the byte length or hash of the received request and compare the exact result, including quotes, dollar signs, backticks, newlines and non-ASCII text. Observe a second distinct reply after another `blueprint input`, and check Runs for exchange/action counts. A CLI admission acknowledgement alone does not prove delivery.
```bash
codeterm blueprint stop <INPUT_GRAPH_ID>
codeterm blueprint stop <OUTPUT_GRAPH_ID>
```
## Worked authoring examples
### Export and import
```bash
codeterm blueprint export <id> --out watch.blueprint.json
codeterm blueprint import watch.blueprint.json --set binding_1=<value> --set timeout=1h --stopped
```
### Wait for CI completion with a model
```bash
codeterm blueprint create --name ci-finished-watch --probe "gh run view 456 --json status,conclusion" --question "waiting while CI is in progress; finished when CI has completed with any conclusion" --buckets waiting,finished --every 20s --until finished --timeout 1h --model-classification
```
### Observe a nightly cron run
```bash
codeterm blueprint create --name nightly-run-watch --observe cron-run:nightly --question "identify the current run state" --buckets waiting,finished --every 20s --timeout 1h --model-classification
```
### Match a stable CI verdict
```bash
codeterm blueprint create --name ci-result-watch --probe "check-ci" --question "CI result" --buckets waiting,passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --every 20s --until passed --timeout 1h
```
### Chain CI to review
```bash
codeterm blueprint create --name "Review stage" --paused --probe "check-review" --question "done when review completes" --buckets done --match 'done=(?m)^done$' --until done --timeout 1h --json
codeterm blueprint create --name "CI and review" --probe "check-ci" --question "passed or failed CI" --buckets passed,failed --match 'passed=(?m)^success$' --match 'failed=(?m)^failure$' --route passed=blueprint:sentinel_DOWNSTREAM_ID --route passed=notify --route failed=stop --timeout 1h
```
Replace `sentinel_DOWNSTREAM_ID` with the exact ID returned for the review stage.

### Named review and stop handlers
```json
{"passed":[{"id":"review","type":"blueprint","parameters":{"target":"sentinel_DOWNSTREAM_ID"}},{"id":"notice","type":"notify","parameters":{}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```
### Command followed by review
```json
{"passed":[{"id":"marker","type":"command","parameters":{"command":"printf marker > marker.txt","label":"Write marker","timeout_ms":1000}},{"id":"review","type":"blueprint","parameters":{"target":"sentinel_DOWNSTREAM_ID"}}],"failed":[{"id":"finish","type":"stop","parameters":{}}]}
```
### Prompt-answer handler and command fixture
Use the same file form to declare a prompt answer:
```json
{"ready":[{"id":"answer_permission","type":"respond_prompt","parameters":{"target":"permission"}}]}
```
For the command fixture, save the **Command followed by review** JSON as `handlers.json` in your disposable directory, replace the downstream ID, and create from there:
```bash
codeterm blueprint create --name "Marker and review" --probe "printf passed" --question "passed or failed" --buckets passed,failed --match 'passed=(?m)^passed$' --match 'failed=(?m)^failed$' --handlers-file handlers.json --timeout 1h
```
### Escalate an unknown result
```json
{"other":[{"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}}]}
```
<!-- revision: 9 -->
