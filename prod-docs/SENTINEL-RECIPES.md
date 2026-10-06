# Sentinel recipes

Ready Sentinel graph patterns and skeletons. Sources, classification, routes, handlers, escalation, updates and limits
are in `codeterm docs SENTINELS`. Name each graph by meaning: what is watched → what happens.

## Use-case patterns

Each pattern names the source, buckets and handlers; put silent buckets as `[]` and check the result with `--dry-run`.

1. **CI or PR result.** `--probe "gh run view ID -R OWNER/REPO --json status,conclusion"`, buckets `passed,failed`, the question naming running CI and probe errors as `other` (without `-R` outside a checkout, gh's error can be read as `failed`); `--route passed=cron:<review>` and `failed=send:<fixer tab>`. The branch is decided once, no polling.
2. **A Worker waiting on a prompt.** `--observe tab:<worker>`, buckets `prompt,busy` in `--handlers-file`: `prompt` to `send:<owner tab>` or `notify`, `"busy": []` for working or idle. Do not escalate `other` here: the hold would hide the next prompt until resolved. The tab is read, never typed into.
3. **An agent finished its turn.** Instead of re-reading `tab get-text`: `--observe tab:<agent>`, buckets `working,idle,prompt`, `"working": []`, `idle` and `prompt` to `notify`, and `--until idle`.
4. **Deploy or service health.** `--probe "curl -fsS --max-time 2 https://HOST/health"` (`curl.exe` on Windows), buckets `healthy,down`, `down=notify`. Only a change of bucket routes; an unassigned `healthy` uses `notify`, so you also wake on the first read and on recovery (`"healthy": []` keeps it silent). To wait for a server to come up, declare only `ready` with `--until ready`: refusals stay `other`.
5. **Long build, test or QA batch.** Have the job write `passed` or `failed` to a status file; `--probe "cat out/test.status"`. While the file is missing the result is a quiet `other`; the default `notify` wakes the creator on each result instead of re-reading a log each turn.
6. **Threshold.** Compute the verdict inside the probe and print one word, e.g. in PowerShell `if ((Get-PSDrive C).Free -lt 20GB) {"low"} else {"ok"}`, with `"ok": []` and `low` to `notify`: you wake when the value crosses, not on every reading.
7. **Self-improving loop.** Escalate `other` to the owning agent (`escalate:agent:<tab>` in `--handlers-file`). It reads `codeterm sentinel history <id>`, adds the missing bucket or handler with `codeterm sentinel update ... --cause <transition>`, then resolves with `codeterm sentinel escalation resolve <id> <transition>` so the stage re-evaluates once. Each unknown case costs one turn, then stays a versioned branch. The update file is not the `history` definition: it takes `name`, `question`, `buckets`, `every`, `handlers` (the create file form), `on_other`, `rearm_on_change`, `escalation_timeout_ms`, `max_transitions` and `max_actions`; the probe or observed source cannot change.
8. **Watching a whole Team.** A General Agent probes `codeterm tab status --tab <orchestrator-tab>` (it reports `not found` once the tab is gone; `orchestrator status` does not) and `codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>` filtered to its `live_lanes` and `drift` lines (the reminder line changes every tick). Buckets `healthy,stuck,needs_owner,lost` in `--handlers-file`: `"healthy": []`; `stuck`, `needs_owner` and `lost` each `notify` the creator once per change; leave `other` quiet, since an escalation hold would hide later changes. A `send` target must share the creator's Team, so the General Agent relays `stuck` to the Team Orchestrator with `codeterm send` and takes `needs_owner` to the owner.

## Built-in recipes

`codeterm sentinel recipe` lists these skeletons. `codeterm sentinel recipe <name>` prints the handlers JSON and create
command; fill in the probe, question and target placeholders, then use `--dry-run` before creating the graph.

<!-- sentinel-recipe:team-watch -->
### team-watch

**Purpose:** Observe one Team member's tab and notify when the watched state changes.

```json
{"waiting":[],"changed":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name team-member-watch --observe 'tab:<TAB_ID>' --question classify-member-state --buckets waiting,changed --match 'changed=<CHANGED_PATTERN>' --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:wait-for-build -->
### wait-for-build

**Purpose:** Wait for a build verdict and notify on either terminal result.

```json
{"waiting":[],"passed":[{"type":"notify","parameters":{}}],"failed":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name build-result-watch --probe "<READ_ONLY_BUILD_PROBE>" --question classify-build-result --buckets waiting,passed,failed --match 'passed=<PASSED_PATTERN>' --match 'failed=<FAILED_PATTERN>' --until passed --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:retry-until-pass -->
### retry-until-pass

**Purpose:** Keep watching a bounded retry process until its pass result arrives.

```json
{"waiting":[],"passed":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name retry-pass-watch --probe "<RETRY_STATUS_PROBE>" --question classify-retry-result --buckets waiting,passed --match 'passed=<PASSED_PATTERN>' --until passed --timeout 1h --handlers-file handlers.json --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:prompt-responder -->
### prompt-responder

**Purpose:** Request a safe response to a supported permission prompt on one observed agent tab.

```json
{"permission":[{"type":"respond_prompt","parameters":{"target":"permission"}}]}
```

```bash
codeterm sentinel create --name permission-prompt-reply --observe 'tab:<TAB_ID>' --question classify-current-prompt --buckets permission --match 'permission=<PERMISSION_PATTERN>' --until permission --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:cron-run-watch -->
### cron-run-watch

**Purpose:** Observe one named cron's run and notify when it finishes.

```json
{"waiting":[],"finished":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name cron-run-finished-watch --observe 'cron-run:<CRON_NAME_OR_ID>' --question classify-run-state --buckets waiting,finished --match 'finished=<FINISHED_PATTERN>' --until finished --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:launch-prompt-gate -->
### launch-prompt-gate

**Purpose:** Answer each scheduled run's launch prompts through the safe-response path; escalate any other prompt to the creator.

```json
{"clear":[],"trust":[{"type":"respond_prompt","parameters":{"target":"directory_trust"}}],"provider":[{"type":"respond_prompt","parameters":{"target":"onboarding"}},{"type":"respond_prompt","parameters":{"target":"permission"}}],"update":[{"type":"respond_prompt","parameters":{"target":"update"}}],"other":[{"type":"escalate","parameters":{"target":"creator"}}]}
```

```bash
codeterm sentinel create --name launch-prompt-gate --observe 'cron-run:<CRON_NAME_OR_ID>' --question classify-launch-prompt --buckets clear,trust,provider,update --match 'clear=\x22prompt\x22:null' --match 'trust=\x22kind\x22:\x22directory_trust\x22' --match 'provider=\x22kind\x22:\x22provider\x22' --match 'update=\x22kind\x22:\x22manual_update\x22' --rearm-on-change trust --rearm-on-change provider --rearm-on-change update --handlers-file handlers.json --escalation-timeout-ms 1800000 --timeout 30d --dry-run
```
<!-- /sentinel-recipe -->

Patterns match the observed prompt kind (`\x22` is a shell-safe regex quote); rearm answers each new prompt. Onboarding
and permission share `provider`, so the handler not matching the live screen records `sentinel_prompt_target_refused`.
Owned launch tabs answer trust and onboarding without a graph through the same writer, one answer per prompt; the graph
adds a journaled branch per prompt and escalates the rest.

<!-- sentinel-recipe:all-finished -->
### Several agents or jobs all finished

**Purpose:** Use one verdict probe for all work; keep `waiting` silent and route only `alldone`.

```json
{"waiting":[],"alldone":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name agents-all-finished-watch --probe "<ONE_VERDICT_PROBE>" --question classify-all-work --buckets waiting,alldone --match 'alldone=<ALL_TERMINAL_PATTERN>' --until alldone --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

<!-- sentinel-recipe:voice-note-to-text -->
### voice-note-to-text

**Purpose:** Convert a voice note to text and classify the transcript.

```json
{"done":[{"type":"notify","parameters":{}}]}
```

```bash
codeterm sentinel create --name voice-note-to-text --input audio --input-file '<PATH_TO_VOICE_NOTE>' --convert-to text --question what-does-the-voice-note-say --buckets done --handlers-file handlers.json --timeout 1h --dry-run
```
<!-- /sentinel-recipe -->

## CI result routed to review or repair

For quiet PR waiting, declare only terminal outcomes; classify still-running CI as `other`, and add an `other`
escalation handler when it needs attention. With an existing same-Team cron and agent:

```bash
codeterm sentinel create --name ci-result-review-watch --probe "gh run view 456 --json status,conclusion" --question "passed for completed successful CI; failed for completed unsuccessful CI; other while running" --buckets passed,failed --route passed=cron:review_pr --route failed=send:repair_worker --timeout 1h
```

<!-- revision: 2 -->
