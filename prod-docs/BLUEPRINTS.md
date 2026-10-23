# Blueprints

Recall saved solutions before authoring, then reuse a matching graph or start from a built-in. Native Blueprint files use `schema_version: 2` and one typed graph. Older formats are refused; stored v1 graphs and saved v1 Blueprint solutions are deleted without conversion. Other memory and workspaces are retained.

```bash
domios blueprint new --like "watch CI and report failures"
domios blueprint recipe
domios blueprint new http-health > health.yaml
domios blueprint check health.yaml --set url=https://example.com/health
domios blueprint apply health.yaml --set url=https://example.com/health --dry-run
domios blueprint apply health.yaml --set url=https://example.com/health
```

`new <recipe|watch|pipeline|command>` prints native YAML. `check` and `apply` accept a recipe name, filename or `-` for stdin. `create` aliases `apply`. Scaffolding, checking and schema output are offline; applying additionally checks live Team scope, targets and installed capabilities. An offer never starts a graph.

## Native v2 documents

```yaml
schema_version: 2
kind: pipeline
name: Service health
note: Probe a service and report its health
intent: watch an HTTP endpoint for outages
owner: team
inputs:
  url: {type: string, description: Service health endpoint}
trigger:
  type: schedule
  every: {lit: 1m}
steps:
  - id: health
    uses: http
    with:
      url: {input: url}
      timeout_ms: {lit: 3000}
    routes: {up: '@stop', down: '@stop', failed: '@stop'}
result: {to: notify}
```

Metadata includes `name`, a printable `note` of at most 200 characters, optional `intent` and `anchors`, typed `inputs` and named port `types`. `kind` is `pipeline` or `watch`. Pipeline ownership is `team` or `tab`; a watch remains scoped to its creator and caller Team. Nodes have stable, unique `id` values; array order fixes execution order. Routes name a later node, `@next` or `@stop`. Use `routes`, never `on`.

Values in `with`, `run`, timing fields and call arguments have exactly one form:

| Form | Meaning |
|---|---|
| `{lit: value}` | Author-fixed JSON data or executable text |
| `{input: name}` | An author input supplied before apply |
| `{bind: node.out.field}` | Runtime data from a preceding node |

There is no expression evaluation or executable template expansion. A `run` input supplies the entire fixed command, rather than inserting data into a command. `name: '{{name}}'` may refer to one whole string input. Literal command text, including braces, remains literal. Non-string `--set key=value` values use JSON syntax. Unknown, duplicate or missing input assignments refuse.

Ports use the restricted JSON Schema fields `type`, `properties`, `required`, `additionalProperties`, `enum`, `items`, `maxLength` and `x-media`, or `{$ref: TypeName}`. Inputs additionally support `default` and `description`. Typed `in`/`out`, `bind`, capability `caps`, code metadata, local `imports`, inline `blueprints` and pipeline `other` handlers are represented in the document and its canonical graph/diff projections.

## Execution support

Admission lowers a v2 document to the runtime representation or returns an explicit unsupported diagnostic before effects. Code and call nodes execute through the shared typed runtime. Runtime data binds resolve retained outputs as data. Lowering refuses any unsupported node or contract before effects; pipeline `other` handlers remain unavailable. `domios-cli` capability is unavailable.

A Blueprint code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4). Code runs as an ordinary child process on macOS, Linux and Windows, subject to execution budgets.

Supported pipeline nodes are `code`, fixed `run` commands and `uses: http`, `github`, `ask`, `filter`, `convert` and `agent`. Parameters are explicit value forms. `ask` declares its `buckets` on the node. HTTP outcomes are `up`, `down`, `failed`; GitHub outcomes depend on `resource`; filters may declare a bucket. Commands, conversion and agent tasks return `done` or `failed`. Unspecified failure routes stop; other unspecified outcomes continue. Inputs and outputs must agree with the supported media chain. `result.to` accepts `owner`, `team`, `notify` or `tab:<id>`.

Command steps default to a 10-second runtime on every platform. Override it on the node with `timeout: {lit: 30s}` (also accepts an author input). The timeout must be positive and at most 60 seconds; invalid values receive a positioned checker diagnostic. For example:

```yaml
- id: inspect
  run: {lit: git status --short}
  timeout: {lit: 30s}
```

Watcher probes also default to 10 seconds on every platform. Each watcher runs concurrently, so a slow probe delays its own verdict while the scheduler and other watchers continue. Command and probe runtime budgets start when the direct child's spawn call returns, excluding its exec authorization delay. Runtime is followed by bounded cleanup of up to 500 ms (one quarter of a shorter budget): the total is runtime plus that reserve, capped by the caller's remaining run deadline. A shorter caller deadline reserves cleanup within that total and may interrupt execution earlier; expiry produces a typed `Deadline`.

A schedule trigger has `every: {lit: 20s}`. Input triggers have a media `kind` and a `source` literal containing `text`, an absolute `file` path or an HTTP(S) `url`; a string source denotes text. Event triggers name an installed capability kind and explicit parameters. Runtime availability is checked at apply.

## Code nodes

Python bodies define `def main(value):` and return a JSON value. TypeScript bodies define `export default function main(value)` (optionally async). The host supplies a fixed harness per language; author code is a separate file. The harness reads the bounded input envelope, calls `main` with its `value`, and writes the returned value with the node’s output media and schema. Text uses a string `value` and an empty `text` field, so it is carried once. Author prints go to stderr. Raw envelope scripts are unsupported.

```yaml
- id: swatch
  code: {lang: python, src: 'def main(value): return "linen"'}
  out: {type: string}
- id: caption
  code: {lang: python, src: 'def main(value): return value.upper()'}
  bind: {in: swatch.out}
  in: {type: string}
  out: {type: string}
```

`code.src` is bounded to 16 KiB. Alternatively, `code.file` names a relative regular file beneath the apply caller’s cwd; symlinks and parent traversal refuse. Apply freezes its bounded bytes into `code.src` before persisting the graph. Later file edits do not change the stored body. Canonical nodes and execution summaries carry `body_sha256`. Runtime refuses unsealed `code.file` and missing `main`.

Python and TypeScript code nodes run as ordinary child processes on macOS, Linux and Windows, using Python or Node from the filtered PATH. Python uses the fixed `main(value)` harness; TypeScript uses Node's type stripping and the fixed exported `main(value)` harness.

Blueprint code and command steps run with the user's own permissions, like an agent in a tab, without Domios credentials in their environment. They can access the user's files, network and subprocesses. Foreign Blueprints are gated by folder trust (1.15.4); inspect their code and commands before granting trust.

`wall_ms` defaults to 3000 for code; `output_bytes` defaults to 32768 and bounds combined stdout/stderr during capture. There is **no default `mem_mb`**. An explicit code memory cap uses `RLIMIT_AS` on Linux and the existing process Job's memory limit on Windows. On macOS, the checker emits the positioned warning `mem_mb not enforced on this OS`; checking, admission and execution continue with deadline and output limits. Memory budgets on other nodes, output budgets on HTTP/agent nodes and watch limits refuse. Command/GitHub output budgets and pipeline wall deadlines remain enforced.

Ordinary exceptions and non-zero code exits produce routable `failed`, reason `code_failed`, with a bounded, redacted stderr tail. Invalid contracts, deadlines and exhausted output budgets stop before routing further effects.

The stdin envelope remains bounded to 32 KiB. Plain text of 20 KiB passes intact. Oversized serialized payloads become an explicit JSON truncation value with `type: truncated`, `truncated: true`, `total_bytes`, `limit_bytes` and a bounded UTF-8 `prefix`. Typed contracts validate that delivered value and may refuse a truncation marker that violates their schema.

Command steps and watcher probes preserve the daemon's inherited PATH order, remove the explicitly configured Domios CLI directories, and append missing default tool directories. Other inherited variables are excluded; children receive `LANG` and, on Windows, `SystemRoot`.

## Watches and effects

```yaml
schema_version: 2
kind: watch
name: Build result
note: Notify when the build passes
steps:
  - id: source
    uses: probe
    with: {command: {lit: echo passed}}
    every: {lit: 20s}
    timeout: {lit: 1h}
  - id: decide
    uses: ask
    with: {question: {lit: Did the build pass?}}
    buckets: [waiting, passed]
    matches: {passed: {lit: '(?m)^passed\r?$'}}
    until: passed
    routes: {waiting: '@stop', passed: alert}
  - id: alert
    uses: notify
```

A watch starts with one source and an `ask` decision. Sources are `probe` with a fixed `command`, `observe` with `kind` (`tab` or `cron_run`) and `id`, or `input` with a media `kind` and `text`, absolute `file` or `url`. An input source can request `convert_to` and `convert_timeout_ms`. Terminal observations use `uses: terminal` with tab binding, `pattern` and/or `idle_ms`.

Source `every` defaults to 20 seconds and is at least 10 seconds. Durations use `s`, `m`, `h`, `d` or combinations, bounded to 366 days. `timeout`, `paused` and `session` belong on the source. The decision owns `matches`, `until`, `rearm_on_change`, `model_classification` and `escalation_timeout_ms`. Keep waiting silent by routing it to `@stop`; this ends that watch's action chain without stopping future observations.

Effects are nodes: `notify`, `send`, `cron`, `blueprint`, `respond_prompt`, `escalate` and `stop`. Targeted effects use `with: {target: {lit: ...}}`; a `done` route can chain later effects. An `other` route may reach notify/escalate nodes and requires a positive escalation timeout on the decision. Only unknown cases escalate. Prompt-response effects request the provider-declared safe path; they do not select keys or options. Commands and targets stay author-fixed and scope-bound. Claims persist before effects, and uncertain delivery never authorizes replay.

## Checking, apply and sharing

```bash
domios blueprint schema > blueprint.schema.json
domios blueprint check health.yaml --set url=https://example.com/health --json --explain
domios blueprint check health.yaml --fix
domios blueprint check health.yaml --fix --write
domios blueprint export <id> --out shared.yaml
domios blueprint import shared.yaml --set command="echo ready"
domios blueprint update <id> --expect-version 3 --from-file health-bound.yaml --dry-run
domios blueprint show <id>
domios blueprint history <id>
domios blueprint log <id>
```

The editor schema is generated from the same serde model as parsing. One checker governs offline check and live apply/import/update. Diagnostics include stable rule and atom ids, file/line/column/field, severity, message, hint, passing/failing premises and a proof tree; dependent findings are folded under the erroneous root. `--explain` renders those proofs; `--json` retains them structurally. Invalid files exit 2. Unsupported runtime constructs refuse with an explicit diagnostic; apply with `--dry-run` also checks live Team scope and installed capabilities.

`--fix` previews text and semantic changes only after the candidate passes checking and runtime lowering with the supplied `--set` bindings. `--fix --write` writes only that admissible candidate. Repairs can declare unambiguous missing inputs, normalize literal durations below 10s with a timing warning, and route watch `other` outcomes to creator escalation. Pipeline escalation repairs, unresolved inputs, ambiguous types and executable/authority repairs refuse without changing the file. R13 compares abstracted canonical documents and WL structure against saved solutions and live Blueprints in the caller's Team; vector comparisons without candidate embeddings are explicitly unavailable.

An exact name identifies a graph in its Team/workspace. Built-in names stay stable when bindings change; apply retargets that name. Use an explicit whole `name` input in custom recipes when distinct identities are required. Applying unchanged running definitions returns `unchanged`; applying a stopped graph reactivates the same ID. Watch lifetime and initial pause settings cannot change through apply. A watch and pipeline cannot replace one another under the same name.

Export generalizes sensitive commands, source data and local targets into described required inputs, excluding runtime state and history. Import uses the same v2 admission and starts stopped unless `--start` is passed. `update --from-file` accepts a native v2 document with its required inputs already bound or defaulted; it enforces the expected version and optional cause. Internal history JSON is not an authoring format. Stopped graphs retain their history for 30 minutes before cleanup.

## Saved solutions

```bash
domios blueprint keep <id> --when "Watch CI and report failing jobs early" --when "tell me when a pull request goes red" --when "ping me once a check fails"
domios blueprint apply memory:<recipe-id> --set key=value
```

Only the Team Orchestrator keeps solutions. Supply one to five intents, preferably three to five distinct requests. Generalization preserves typed required inputs and removes instance defaults. Existing vector recall, per-intent matching, semantic canon, structural signatures, enrichment provenance and bounded variant merging remain in use. CLI offers preserve the server's ranking. A save retains its own entry and reports archived IDs when the area storage cap applies. Worker unavailability leaves enrichment pending without failing the save.

Full recipe purposes and inputs: `domios docs BLUEPRINT-RECIPES`.

Save a triggerless typed callable without scheduling it:

```bash
domios blueprint keep --file increment.yaml --when "Increment a typed counter"
domios blueprint show mem:<solution-id> --json
```

Keep preserves the callable input signature and returns its stable `mem:` identity and hash. Reuse that identity in `call.ref` with structural `lit`, `input`, or `bind` arguments. The CLI reads relative local imports from each authoring file and sends their document bytes; the host resolves nested `mem:` calls within the caller's Team and workspace. Saving or applying freezes the exact transitive closure. Existing pins stay fixed. Edits, merges, archives and area-cap eviction produce a warning with a reason and semantic changes; archived heads cannot receive new pins. Missing pinned bytes refuse execution. Callable files use `keep --file`; `apply` admits scheduled graphs. `show` includes expanded pinned callee bodies and bounded nested run results, stops and call stacks. Historical runs use their retained definition; an older run without that definition explicitly shows the historical graph as unavailable. Call checks require host resolution and use the same admission checker as apply. Callable checks preserve required input signatures and schedule nothing. Import and update carry the same frozen local bundle; exported pins stay fixed. `check --fix` explicitly refuses call documents; use `check` without `--fix`.

Callable `other` handlers are refused at check time; move failure handling to the caller. Inside callees, notify/send/cron/escalate use caller scope and the shared executor claim. Blueprint activation, stop and prompt-response effects require scheduler custody and are refused at check time inside calls; put those effects in the caller.

The simulation harness is internal and has no CLI verb. It records bounded delivery provenance in memory with zero delivery and no real run journal. Process, code, agent, HTTP and conversion nodes refuse before launch; pure filters and calls share the normal budget and port checks.

Blueprint results addressed to a restored PTY agent tab retain the exact saved provider and session. The durable outbox holds them for the first authenticated launch of that session, with a fixed 24-hour delivery TTL. Held messages retain the original run id and timestamp; live delivery keeps its existing body. A closed recipient, changed session or provider, failed launch, or expired TTL produces a durable failure receipt in the producer's AgentPad and a status event; a live producer also receives a failure notification. Failure receipts are idempotent by delivery id, and retained terminal states recover their original cause across a crash. Recovery re-emits the status by delivery id if settlement was interrupted; an externally edited receipt is preserved without blocking later results. A result that may already have been written remains uncertain and is never replayed. Restored chat-backend delivery uses its existing reopen path and reports reopen failures; it does not yet use this durable hold.

R10 scans executable text lexically as defence in depth. The checker refuses domios-cli capabilities, while credential-free child environments remove instance authority; a clean lexical scan does not prove shell text safe. R13 reports unavailable comparisons explicitly; unavailable vector comparisons carry no PASS verdict.

<!-- revision: 58 -->
