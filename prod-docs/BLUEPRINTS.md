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

Admission lowers a v2 document to the runtime representation or returns an explicit unsupported diagnostic before effects. Typed ports and a preceding node’s `out` data binding are supported on code inputs. Calls, pipeline effect nodes, pipeline `other` handlers and unsupported capability or budget fields refuse before effects. `domios-cli` capability is unavailable.

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

Code execution requires macOS Seatbelt and a supported Python framework or Apple Silicon Homebrew interpreter. Windows, Linux and Intel Homebrew code execution refuse explicitly. The existing sandbox denies network, child processes and filesystem data outside the interpreter and private temp roots. No instance credentials enter its environment. TS launches once with type stripping and string code generation disabled; unsupported Node flags refuse.

`wall_ms` defaults to 3000 for code; `output_bytes` defaults to 32768 and bounds combined stdout/stderr during capture. There is **no memory cap by default**. `limits.mem_mb` on macOS code is refused at check with `unsupported: memory budget on macOS`; no spawn failure counts as memory enforcement. Memory budgets on other nodes, output budgets on HTTP/agent nodes and watch limits also refuse. Command/GitHub output budgets and pipeline wall deadlines remain enforced.

Ordinary exceptions and non-zero code exits produce routable `failed`, reason `code_failed`, with a bounded, redacted stderr tail. A permission-denial signal from the harness or Seatbelt launcher stops with `sandbox_denied`, including Python `PermissionError` caught by the body while tracing is active. These harness observations are diagnostic evidence; author code can disable language hooks. Seatbelt remains the confinement boundary. Missing confinement, invalid contracts, deadlines and exhausted output budgets stop before routing further effects.

The stdin envelope remains bounded to 32 KiB. Plain text of 20 KiB passes intact. Oversized serialized payloads become an explicit JSON truncation value with `type: truncated`, `truncated: true`, `total_bytes`, `limit_bytes` and a bounded UTF-8 `prefix`. Typed contracts validate that delivered value and may refuse a truncation marker that violates their schema.

Command steps and watcher probes preserve the daemon's inherited PATH order, remove the explicitly configured Domios CLI directories, and append missing default tool directories. Other inherited variables are excluded; children receive `LANG` and, on Windows, `SystemRoot`. PATH filtering is not a confinement boundary.

Current hardening limitations: metadata/readlink access and root-directory listing remain broader than data reads, and `sysctl-read` remains unrestricted. These can disclose host metadata. Inherited descriptors rely on host `O_CLOEXEC`; no `closefrom` hardening is claimed. The profile has not been widened to support additional interpreters or dylib roots.

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

R10 scans executable text lexically as defence in depth. Runtime environment clearing and capability confinement enforce authority; a clean lexical scan does not prove shell text safe. R13 reports unavailable comparisons explicitly; unavailable vector comparisons carry no PASS verdict.

<!-- revision: 52 -->
