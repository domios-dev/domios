# Blueprints

Author a Blueprint as one YAML file, or apply a built-in YAML recipe directly. Check it offline before applying it to your Team.

```bash
domios blueprint apply github --set repo=owner/repo --set id=42 --note "Report CI failures"
domios blueprint new http-health > health.yaml
domios blueprint check health.yaml --set url=https://example.com/health
domios blueprint apply health.yaml --set url=https://example.com/health --dry-run
domios blueprint apply health.yaml --set url=https://example.com/health
```

`recipe` lists built-ins. `new <recipe|watch|pipeline|command>` prints a commented file. `check` and `apply` accept either a recipe name or a filename (`-` reads stdin). Built-in recipes are the same files that `new` prints. `create` is an alias of `apply`; the old creation flags have been removed.

## Reuse a saved solution

`domios blueprint new --like "watch CI run and ping me on failure"` and `domios blueprint apply --like "watch CI run and ping me on failure"` offer matching saved solutions beside built-ins without starting them. `domios blueprint recipe` lists built-in names and top saved solutions for the current project workspace. Reuse a saved solution with `domios blueprint apply memory:<recipe-id> --set key=value`. Each offer includes YAML and described typed inputs; instance inputs have no defaults. Named built-in recipes remain available offline.

Nothing is saved automatically. The Team Orchestrator queues a generalised solution with 3 to 5 `--when` intents, each a different way someone would ask for it:

```bash
domios blueprint keep <id> --when "Watch CI and report failing jobs early" --when "tell me when a pull request build goes red" --when "ping me once a check fails"
```

`--when` repeats 1 to 5 times. Workers cannot keep solutions. Each intent is searched on its own and the best match counts, so vary the wording rather than repeating one phrase. A short note explains the graph, and concrete commands, URLs, repository names, IDs and personal paths become required inputs. Instance values and credentials are never retained as defaults. Safety checks still apply to explicit keeps. Admission returns `queued`; debug output records the save decision or refusal.

Recall also searches a description derived from the graph itself: the note, step names and a gist of the commands, URLs and tools, never routes or step numbers. After a keep, a hidden memory worker on `memory.worker_model` may add a summary and extra intents from the stored YAML; that text only helps search, records its model and document hash, and is regenerated when the graph changes. When workers are unavailable the keep still succeeds and the entry stays `enrichment pending` until a later backfill. Each offer carries a `why` line naming the matching intent and scores.

`blueprint new --like` prints the selected solution’s when-to-use line and match reason. Its JSON response includes `offers`, `degraded` and re-embedding `progress`, even when no solution matches.

Solutions are memory skills, discoverable through ordinary memory search. Equivalent structures update one recipe's statistics. Ranking and eviction consider successes, twice the reuse count, an explicit-keep bonus and failures, with age decay. Each area retains at most 1000 active entries as a storage bound; recall ranks the stored library before applying the requested result limit. A save always retains the entry being saved and returns `evicted_ids` for any archived entries; the CLI prints each archived ID. Archived entries preserve their YAML. The Dreamer considers at most 1000 entries in one area and merges at most eight pairs per cycle; it never creates solutions from runs. It can log a built-in promotion proposal. Background writes use a bounded queue; keep waits for the committed save receipt before returning while indexing continues in the background; debug logs show items added today and recall hit rate.

## One file

```yaml
schema_version: 1
name: Service health
note: Diagnose outages and report the result
owner: team
inputs:
  url:
    type: string
    description: Service health endpoint
    default: https://example.com/health
pipeline:
  trigger: {type: schedule, every: 1m}
  steps:
    - name: health
      input: text
      uses: http
      with: {url: '{{url}}', timeout_ms: 3000}
      on: {up: stop, down: diagnose, failed: diagnose}
    - name: diagnose
      input: text
      run: echo inspect-service
      on: {done: stop, failed: stop}
  result: {to: notify}
```

Use exactly one `pipeline` or `watch`. Metadata stays at the top: `schema_version: 1`, `name`, optional `note` (at most 200 printable characters), optional pipeline `owner` (`team`, the default, or `tab`), and `inputs`. Watches belong to their creator in the caller's Team and reject an explicit `owner`.

`inputs` declares types, descriptions, optional defaults and optional `enum` choices. Types are `string`, `integer`, `number`, `boolean`, `object`, and `array`. Supply required inputs with `--set key=value`; non-string values use JSON syntax. A whole-value `'{{key}}'` preserves its type; embedded inputs must be scalars. Spaces in `'{{ key }}'` are allowed. Only declared input names substitute; unrelated braces in commands stay literal. Inputs never expand mapping keys or execute templates. Includes and packages are unsupported.

## Check and editor validation

```bash
domios blueprint schema > blueprint.schema.json
domios blueprint check health.yaml --set url=https://example.com/health --json
```

Scaffolds start with a `yaml-language-server` hint referring to `./blueprint.schema.json`. Save the schema beside the YAML file for editor validation. `check` remains authoritative: it accumulates structural and semantic errors, including duplicate keys, missing fields, unknown nodes, invalid durations, route targets and unreachable stages. Each diagnostic contains file, line, column, field, severity, message and a fix hint. Invalid files exit 2. Missing notes and hardcoded GitHub run IDs produce warnings and do not fail the check.

Checking, scaffolding and schema output require no daemon or network. Plugin/converter availability, live targets, permissions and Team scope are checked when applying; an offline check cannot prove those runtime conditions.

## Apply by name

Built-in recipe names use short identities. HTTP names use host and path, excluding credentials, query and fragment. Different paths create separate Blueprints; rotating query tokens updates the same Blueprint. Probe-based watch recipes require a short `name` input. Long expanded names use a slug prefix and a stable six-character hash within the 40-character bound.

`apply` requires a live caller in a Team. An exact name in that Team and workspace identifies the existing Blueprint. Applying an identical definition returns `unchanged` without adding a version; a change updates that ID through versioned admission. Ambiguous names and unauthorized updates refuse. A watch and a pipeline cannot replace each other under the same name. Watch source and route changes retarget the existing graph. Watch `timeout` and `paused` apply only at creation. Applying changed creation settings refuses with a source-positioned diagnostic; delete and re-apply to change them. Original settings persist through restart. Preexisting watches without recorded original settings require recreation before apply can compare these options.

`--dry-run` validates live admission and prints the proposed action and before/after definitions without mutation. `--note` overrides the document note. Applying proves admission; confirm an actual source execution and recognized outcome with `show`, and confirm a fired route through its retained result.

## Pipelines

A pipeline contains `trigger`, `steps` and optional `result`. Triggers are `{type: schedule, every: 20s}`, `{type: input, kind: text, source: {text: hello}}`, or `{type: event, kind: capability_name, params: {}}`. Event sources come from installed plugins.

Each named step declares its `input` (`text`, `file`, `audio`, `video`, or `image`). Use `run` for a fixed command, or `uses` and `with` for a built-in node:

| Node | Parameters | Outcomes |
|---|---|---|
| `http` | `url`, optional `expected_status` (default `2xx`), `timeout_ms` (default 3000, max 10000) | `up`, `down`, `failed` |
| `github` | exactly one `repo` or `org`, `resource`, optional `id`, `label`, `timeout_ms` (max 60000) | run/jobs: `done`, `failing`, `failed`; PR checks: `done`, `failed`; runners: `online`, `offline`, `failed` |
| `ask` | `question`, `buckets` | declared buckets and `other` |
| `filter` | `pattern`, optional `bucket` | `done`, `failed`, optional bucket |
| `convert` | `from`, `to` | `done`, `failed` |
| `agent` | `provider`, `profile`, `task`, `deadline_ms`, `max_tokens`, `max_turns` | `done`, `failed` |
| `run` | command text directly | `done`, `failed` |

Command nodes output text. Commands run in PowerShell on Windows and sh elsewhere; use portable commands such as `echo`, or typed `http` / `github` sources instead of shell-specific syntax. Their stdin contains one JSON value followed by EOF: a preceding JSON result retains its types, and plain text becomes a JSON string. Input is capped at 32 KiB, like each output stream. Oversized input becomes a JSON object with `type: "truncated"`, `truncated: true`, `total_bytes` (original UTF-8 bytes), `limit_bytes`, and a bounded text `prefix`. Command nodes run without Domios credentials or instance selectors: all inherited `CODETERM_*` / `DOMIOS_*` variables are removed, and the instance CLI directory is removed from PATH. HOME, normal tool paths, locale and temporary-directory settings remain available. Commands may ignore stdin without blocking; model output never becomes executable command text. Agents use a provider with the bounded task capability, return structured status/output, and obey explicit budgets. Graph stop aborts the active task; uncertain effects remain unconfirmed.

`on` maps outcomes to `next`, `stop` or a later named stage. An unspecified `failed` outcome stops the run; other unspecified outcomes go to the next stage. Routes are forward-only. `stop` ends that run. A command that exits unsuccessfully and ends the run as failed also stops the recurring graph; route `failed` to a recovery stage to keep it running. Repair the command and re-apply to resume. `blueprint stop` explicitly stops the recurring graph. `result.to` accepts `owner`, `team`, `notify` or `tab:<id>`. Steps must agree on input/output types. Plugin conversion availability is verified at apply.

## Watches

```yaml
schema_version: 1
name: Build result
note: Notify when the build finishes
watch:
  probe: echo waiting
  question: Does the verdict say waiting, passed or failed?
  buckets: [waiting, passed, failed]
  matches: {passed: '(?m)^passed$', failed: '(?m)^failed$'}
  routes: {waiting: none, passed: notify, failed: notify}
  every: 20s
  until: passed
  timeout: 1h
```

Choose exactly one source: `probe` command, `observe: tab:<id>` / `cron-run:<name-or-id>`, or `input: {kind: text, data: {text: hello}}`. Input data can instead use an absolute `file` path or HTTP(S) `url`; optional `convert_to` and `convert_timeout_ms` request conversion. Runtime media support is checked at apply/execution. Watches convert media to text through an installed converter; apply refuses an unavailable conversion with `input_runtime_unavailable`. If that capability disappears later, the watch is held with the reason visible in `show`. Text stages accept later messages through `blueprint input <id> --from-pad <path>` or `--text-file <path>`; admission is not a delivery receipt. Pending messages are discarded on stop, restart or definition changes.

`every` defaults to `20s` and must be at least `10s`. Durations use `s`, `m`, `h`, `d` or combinations such as `1h30m`, bounded to 366 days. `until` and `rearm_on_change` name declared buckets. A terminal shell observation requires `terminal: {pattern: '...', idle_ms: 500}` with a pattern or idle limit (10..60000ms).

Patterns are deterministic; unmatched or overlapping observations are `other`. `model_classification: true` explicitly enables a connected Supporter Choice adapter when no patterns are declared. Unchanged output calls no model. Probe results include exit status, stdout and stderr; use stable verdict lines, multiline anchors and patterns that handle CRLF. Each probe is bounded to three seconds on Unix or ten on Windows, retaining at most 32 KiB per stream.

Watch `routes` map buckets to `notify`, `stop`, `none`, `send:<tab>`, `cron:<name>` or `blueprint:<exact-id>`. Unassigned buckets notify. For richer actions, use `handlers` instead of `routes`:

```yaml
handlers:
  waiting: []
  failed:
    - type: command
      parameters: {command: echo investigate, label: Investigate failure, timeout_ms: 1000}
  other:
    - type: escalate
      parameters: {target: creator}
escalation_timeout_ms: 60000
```

Handler IDs are optional and generated deterministically; parameters default to `{}`. Types are `notify`, `stop`, `send`, `cron`, `blueprint`, `command`, `respond_prompt`, and `escalate`. Targeted handlers use `parameters.target`. Only `other` may escalate, to `creator` or `agent:<tab>`, and it requires a positive timeout. `other` supports at most four handlers, only notify/escalate, and at most one escalation. Composition watches require an explicit lifetime; transition/action limits default to 64/256 and may not exceed 256/1024.

Actions and targets stay in the creator's Team/workspace. Commands are fixed by the author. Prompt response handlers request the provider-declared safe path; they cannot choose keys or options. Claims are saved before effects and are never replayed after uncertainty or restart. A source removal stops/notifies; CronRun watches unbind finished runs and wait for a verified replacement run.

## Sharing and operation

```bash
domios blueprint export <id> --out shared.yaml
domios blueprint import shared.yaml --set binding_1=value
domios blueprint list
domios blueprint show <id>
domios blueprint history <id>
domios blueprint log <id>
domios blueprint stop <id>
domios blueprint delete <id> --stop
```

Both the UI and CLI export and import the same YAML. Export writes the same authoring YAML with sensitive commands, input data, patterns and local targets replaced by required described inputs. It excludes runtime/history and local owner IDs. Import binds those inputs in the caller's Team and starts stopped by default; `--start` starts immediately. Check exported files after supplying every required input.

Stopped graphs and their history remain for 30 minutes before cleanup. Changed definitions create an immutable version; an unchanged apply reactivates a stopped graph under the same ID. Import uses the same name lookup and keeps the result stopped unless explicitly started. Advanced `update` accepts an internal definition file with `--expect-version` for explicit version/cause control. `history` definitions can be restored through that path, and `escalation resolve <id> <transition>` releases a held unknown case after repair. For a watch, use `show` to inspect its latest observation, progress and transitions; `log` provides pipeline journals. Unscheduled inputs omit `next_run`. Recipe examples: `domios docs BLUEPRINT-RECIPES`.

<!-- revision: 47 -->
