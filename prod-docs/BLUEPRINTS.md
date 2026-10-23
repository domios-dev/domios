# Blueprints

Recall saved solutions before authoring, then reuse a matching graph or start from a built-in. Native Blueprint files use `schema_version: 2` and one typed graph. Older formats are refused; stored v1 graphs and saved v1 Blueprint solutions are deleted without conversion. Other memory and workspaces are retained.

Each code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4). Execution budgets still apply on macOS, Linux and Windows.

```bash
domios blueprint new --intent "watch CI and report failures" --json
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

Admission lowers a v2 document to the runtime representation or returns an explicit unsupported diagnostic before effects. Code and call nodes can be serialized and checked, but their execution is currently refused. Runtime data binds, custom JSON ports, pipeline effect nodes, pipeline `other` handlers and per-node capability/budget enforcement also refuse when lowering cannot preserve their meaning. `domios-cli` capability is unavailable.

Supported pipeline nodes are fixed `run` commands and `uses: http`, `github`, `ask`, `filter`, `convert` and `agent`. Parameters are explicit value forms. `ask` declares its `buckets` on the node. HTTP outcomes are `up`, `down`, `failed`; GitHub outcomes depend on `resource`; filters may declare a bucket. Commands, conversion and agent tasks return `done` or `failed`. Unspecified failure routes stop; other unspecified outcomes continue. Inputs and outputs must agree with the supported media chain. `result.to` accepts `owner`, `team`, `notify` or `tab:<id>`.

A schedule trigger has `every: {lit: 20s}`. Input triggers have a media `kind` and a `source` literal containing `text`, an absolute `file` path or an HTTP(S) `url`; a string source denotes text. Event triggers name an installed capability kind and explicit parameters. Runtime availability is checked at apply.

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

`--fix` previews the actual text and semantic diff. `--fix --write` writes a file; a second fix makes no change. Repairs declare missing inputs only when every use determines one type, route unknown `other` cases to creator escalation, normalize literal durations below 10s with a timing warning, and canonicalize mapping keys. Ambiguous types and executable/authority repairs refuse without changing the file. The checker never grants capabilities; `domios-cli` remains unavailable. Cross-Blueprint duplicate findings use existing scoped vector/WL comparison when an indexed identity is available, otherwise diagnostics explicitly report the unavailable comparison without a lexical substitute.

An exact name identifies a graph in its Team/workspace. Built-in names stay stable when bindings change; apply retargets that name. Use an explicit whole `name` input in custom recipes when distinct identities are required. Applying unchanged running definitions returns `unchanged`; applying a stopped graph reactivates the same ID. Watch lifetime and initial pause settings cannot change through apply. A watch and pipeline cannot replace one another under the same name.

Export generalizes sensitive commands, source data and local targets into described required inputs, excluding runtime state and history. Import uses the same v2 admission and starts stopped unless `--start` is passed. `update --from-file` accepts a native v2 document with its required inputs already bound or defaulted; it enforces the expected version and optional cause. Internal history JSON is not an authoring format. Stopped graphs retain their history for 30 minutes before cleanup.

## Recall before authoring

Start with `domios docs BLUEPRINT-AUTHORING`, the shared short cookbook with a generic `bind.in` example. `blueprint new --intent` returns a minimal native-v2 scaffold with that exact intent and the top three compact typed cards; `--limit` accepts 1..20. `--json` includes `scaffold`, `yaml`, `cards`, `recall` status and `unavailable` entries. Cards carry a stable `mem:` or `catalog:` id, its exact content hash, provenance, score, one-line purpose/WHY, input/result schemas and node-kind counts. They never inline YAML or code. `blueprint show <card-id>` opens a body explicitly. Existing per-intent vectors, canonical descriptions and current enrichment feed one ranked pool for saved solutions and eligible built-in callable nodes. Current intent vectors are mandatory; missing vectors yield visible degraded/pending state, without a word-only substitute. Workspace scope is applied before ranking. Incompatible catalog entries report their exact unavailable native contract.

Reuse a fitting card by a typed `call`; require the complete purpose and port contract to fit. A high score is not a fit decision. No-fit responses retain their scaffold even when irrelevant candidates are returned. Author from scratch when none fits. Call checking returns the sealed native definition, including its `{id, hash}` pins and immutable dependencies; save that returned definition before apply. Run records and `show` expose the expanded callee. Missing revisions refuse and retained pins never silently rebind.

## Saved solutions

```bash
domios blueprint keep <id> --when "Watch CI and report failing jobs early" --when "tell me when a pull request goes red" --when "ping me once a check fails"
domios blueprint apply memory:<recipe-id> --set key=value
```

Only the Team Orchestrator keeps solutions. Supply one to five intents, preferably three to five distinct requests. Generalization preserves typed required inputs and removes instance defaults. Existing vector recall, per-intent matching, semantic canon, structural signatures, enrichment provenance and bounded variant merging remain in use. CLI offers preserve the server's ranking. A save retains its own entry and reports archived IDs when the area storage cap applies. Worker unavailability leaves enrichment pending without failing the save.

Full recipe purposes and inputs: `domios docs BLUEPRINT-RECIPES`.

<!-- revision: 50 -->
