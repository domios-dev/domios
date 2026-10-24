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

`new <recipe|watch|pipeline|command>` prints native YAML. `check` and `apply` accept a recipe name, filename or `-` for stdin. `create` aliases `apply`. Named scaffolds, schema output and checking graphs without calls are offline. `new --intent` recall and typed-call checking use live Team/workspace admission; applying additionally checks live targets and installed capabilities. An offer never starts a graph.

## Repository discovery and human review

From the repository root, list and view files under `.domios/blueprints/` without applying them:

```bash
domios blueprint project list --json
domios blueprint project show .domios/blueprints/health.yaml --json
```

Discovery reports the source path, name and intent. Names must exactly match file stems; linked or escaping paths, duplicate stems across extensions and files larger than 64 KiB refuse. This surface only reads: listing, viewing and repository changes never run, apply or approve a Blueprint. Repository execution requires its separate trust approval.

The Jobs view offers the same repository list and source viewer. Canvases show typed nodes, ports and forward routes; select a call node for its retained id/hash, inputs, result and body, or a run for its own journal and nested stop evidence (Stop selected run stops the current run). Trigger and Result take their state from the `$run` and `$result` journal rows, and expanded callee nodes carry the callee path. Held nodes and Blueprints show their recorded reason: pause, an uncertain claim/effect/result after a restart, or the journal reason. A Blueprint whose owning workspace, Team or tab is plan-locked is held with `plan_limit_locked`: it observes and runs nothing, and apply, edit and run-now are refused; delete and export remain. Unlock resumes the saved cursor; watches take a fresh baseline and fire only on a later change. Semantic diff compares retained definition versions; parameter fields show CLI `--set` assignments without submitting them.

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

Domios does not evaluate expressions or expand executable templates. A `run` input supplies the entire author-fixed shell command line, rather than inserting runtime data into a command. `name: '{{name}}'` may refer to one whole string input. Domios passes command text unchanged to the shell, which applies its own quoting and expansion. Non-string `--set key=value` values use JSON syntax. Unknown, duplicate or missing input assignments refuse.

Ports use the restricted JSON Schema fields `type`, `properties`, `required`, `additionalProperties`, `enum`, `items`, `maxLength` and `x-media`, or `{$ref: TypeName}`. Inputs additionally support `default` and `description`. Typed `in`/`out`, `bind`, capability `caps`, code metadata, local `imports`, inline `blueprints` and pipeline `other` handlers are represented in the document and its canonical graph/diff projections.

## Execution support

Admission lowers a v2 document to the runtime representation or returns an explicit unsupported diagnostic before effects. Code and call nodes execute through the shared typed runtime. Runtime data binds resolve retained outputs as data. Lowering refuses any unsupported node or contract before effects; pipeline `other` handlers remain unavailable. `domios-cli` capability is unavailable.

A Blueprint code or command step runs with the user's own permissions, like an agent in a tab, without Domios credentials in its environment; foreign Blueprints are gated by folder trust (1.15.4). Code runs as an ordinary child process on macOS, Linux and Windows, subject to execution budgets.

Supported pipeline nodes are `code`, fixed `run` commands and `uses: http`, `github`, `ask`, `filter`, `convert` and `agent`. Parameters are explicit value forms. `ask` declares its `buckets` on the node. HTTP outcomes are `up`, `down`, `failed`; GitHub outcomes depend on `resource`; filters may declare a bucket. Commands, conversion and agent tasks return `done` or `failed`. Unspecified failure routes stop; other unspecified outcomes continue. Inputs and outputs must agree with the supported media chain. `result.to` accepts `owner`, `team`, `notify` or `tab:<id>`; offline checking refuses any other target. With the default `owner: team`, `owner` and `team` deliver to the Team Orchestrator's inbox in the Blueprint's workspace; with `owner: tab`, `owner` delivers to the applying tab.

`run` is a shell command line, not argv; runtime values arrive in its stdin envelope. `bind: {in: prior.out}` passes one value unchanged. Code/command named ports (`bind: {a: first.out, b: second.out}`) pass `{a: <first value>, b: <second value>}` to Python/TS `main(value)` or command envelope `value`. A closed object schema is inferred when `in` is omitted; declared object schemas are checked per port. Use up to 32 nonempty port names, at most 64 bytes without dots; never mix them with `in`. All binds, including call arguments, require data dominance: every path to the consumer must execute its producer, including after merges. Positioned `D1` names the producer and a skipping branch path. Shell details/example: `domios docs BLUEPRINT-AUTHORING`.

Command steps default to a 10-second runtime on every platform. Override it on the node with `timeout: {lit: 30s}` (also accepts an author input). The timeout must be positive and at most 60 seconds; invalid values receive a positioned checker diagnostic. For example:

```yaml
- id: inspect
  run: {lit: git status --short}
  timeout: {lit: 30s}
```

Watcher probes also default to 10 seconds on every platform. Each watcher runs concurrently, so a slow probe delays its own verdict while the scheduler and other watchers continue. Command and probe runtime budgets start when the direct child's spawn call returns, excluding its exec authorization delay. Runtime is followed by bounded cleanup of up to 500 ms (one quarter of a shorter budget): the total is runtime plus that reserve, capped by the caller's remaining run deadline. A shorter caller deadline reserves cleanup within that total and may interrupt execution earlier; expiry produces a typed `Deadline`.

A schedule trigger has `every: {lit: 20s}`. Input triggers have a media `kind` and a `source` literal containing `text`, an absolute `file` path or an HTTP(S) `url`; a string source denotes text.

A `text` trigger delivers a **string**: `{text: "7"}` reaches the first node as the string `"7"`, never the integer 7. The text is parsed as JSON only when the first node's `in` schema rejects a string and the parsed value satisfies that schema (`in: {type: integer}` with `"7"` delivers 7; `in: {type: object, ...}` with `{"n":7}` delivers the object). With no `in` schema the node receives the string. To pass structured data, declare the object schema on the first node's `in` or parse the string in code. A mismatch stops the node with `contract_violation`; its journal summary and `show` name the expected schema and the received media and value (clipped), for example `node stopped: contract_violation: input expected {"type":"integer"} got text "seven"`. Event triggers name an installed capability kind and explicit parameters. Runtime availability is checked at apply.

## Code nodes

`code.lang` is `python` or `ts` (`typescript` is refused). `shell` parses but is not runnable: a `code` body must declare `main`, so shell text belongs in a fixed `run` command step. Limits sit under the code block (`code: {lang: python, src: ..., limits: {wall_ms: 3000, output_bytes: 32768}}`); a node-level `limits` is also accepted. Code nodes run in a throwaway temporary working directory, while command and probe steps run in the Blueprint cwd. Python bodies define `def main(value):` and return a JSON value. TypeScript bodies define `export default function main(value)` (optionally async). A fixed per-language harness reads the bounded input envelope, calls `main` with its `value`, and writes the result with the node’s output media and schema. Text uses a string `value` and an empty `text` field, so it is carried once. Author prints go to stderr. Raw envelope scripts are unsupported.

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

`code.src` is bounded to 16 KiB. Alternatively, `code.file` names a relative regular file beneath the directory of the Blueprint file that declares it. Absolute paths, parent traversal, symlinks and Windows junctions or other reparse points at any component refuse with `contract_violation`, and the resolved file must stay inside that directory; the same reader serves apply and runtime. Apply freezes its bounded bytes into `code.src` before persisting the graph. Later file edits do not change the stored body. Canonical nodes and execution summaries carry `body_sha256`. Runtime refuses unsealed `code.file` and missing `main`.

Python and TypeScript code nodes run as ordinary child processes on macOS, Linux and Windows, using Python or Node from the filtered PATH. Python uses the fixed `main(value)` harness; TypeScript uses Node's type stripping and the fixed exported `main(value)` harness.

Blueprint code and command steps run with the user's own permissions, like an agent in a tab, without Domios credentials in their environment. They can access the user's files, network and subprocesses, including Domios data directories and the instance credentials stored there. Code nodes and command steps both run in the Blueprint's cwd and keep the user's identity, home and temp variables (`USERNAME`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `TEMP`, `TMP`, `APPDATA`, `LOCALAPPDATA` on Windows; `HOME`, `USER`, `LOGNAME`, `TMPDIR` elsewhere); `CODETERM*`/`DOMIOS*` variables and other credentials are dropped. Foreign Blueprints are gated by folder trust (1.15.4); inspect their code and commands before granting trust.

`wall_ms` defaults to 3000 for code; `output_bytes` defaults to 32768 and bounds combined stdout/stderr during capture. There is **no default `mem_mb`**. An explicit code memory cap uses `RLIMIT_AS` on Linux and the existing process Job's memory limit on Windows. On macOS, the checker emits the positioned warning `mem_mb not enforced on this OS`; checking, admission and execution continue with deadline and output limits. Memory budgets on other nodes, output budgets on HTTP/agent nodes and watch limits refuse. Command/GitHub output budgets and pipeline wall deadlines remain enforced.

Ordinary exceptions and non-zero code exits produce routable `failed`, reason `code_failed`, with a bounded, redacted stderr tail. Invalid contracts, deadlines and exhausted output budgets stop before routing further effects.

The stdin envelope remains bounded to 32 KiB. Plain text of 20 KiB passes intact. Oversized serialized payloads become an explicit JSON truncation value with `type: truncated`, `truncated: true`, `total_bytes`, `limit_bytes` and a bounded UTF-8 `prefix`. Typed contracts validate that delivered value and may refuse a truncation marker that violates their schema.

Command steps and watcher probes preserve the daemon's inherited PATH order, remove the explicitly configured Domios CLI directories, and append missing default tool directories. Other inherited variables are excluded; children receive `LANG` and, on Windows, `SystemRoot`, `PATHEXT` for tool-name resolution and a `COMSPEC` pointing to the system `cmd.exe` for batch files.

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

The editor schema and parser share one serde model; offline check and live apply/import/update share one checker. Diagnostics carry stable rule/atom IDs, file/line/column/field, severity, message, hint, premises and proof trees. Dependent findings fold under the root error. `--explain` lists causal premises under `CAUSED BY` and other premises under `GIVEN`; unpositioned live errors name the field. JSON preserves structure. Invalid files exit 2; unsupported constructs refuse explicitly. `apply --dry-run` also checks live Team scope and capabilities.

`--fix` previews text and semantic changes only after the candidate passes checking and runtime lowering with the supplied `--set` bindings. `--fix --write` writes only that admissible candidate. Repairs can declare unambiguous missing inputs, normalize literal durations below 10s with a timing warning, and route watch `other` outcomes to creator escalation. Pipeline escalation repairs, unresolved inputs, ambiguous types and executable/authority repairs refuse without changing the file. R13 compares documents against saved solutions and live Blueprints in the caller's Team. Name, note, intent and anchors are ignored. An `exact` twin has otherwise identical content, or matches a saved solution's generalized body; a `structural_twin` has the same abstracted WL shape and identical code bodies, commands, parameters, trigger source and input defaults. Graphs that differ in code or literals are distinct and never refused; vector comparisons without candidate embeddings are explicitly unavailable.

An exact name identifies a graph in its Team/workspace. Built-in names stay stable when bindings change; apply retargets that name. Use an explicit `name` input when distinct identities are required. Applying unchanged running definitions returns `unchanged`; applying a stopped graph reactivates the same ID. Watch lifetime and initial pause settings cannot change through apply. A watch and pipeline cannot replace one another under the same name.

Export generalizes sensitive commands, source data and local targets into described required inputs, excluding runtime state and history. Import uses the same v2 admission and starts stopped unless `--start` is passed. `update --from-file` accepts a native v2 document with its required inputs already bound or defaulted; it enforces the expected version and optional cause. Internal history JSON is not an authoring format. Stopped graphs retain their history for 30 minutes before cleanup.

## Recall before authoring

`domios docs BLUEPRINT-AUTHORING` covers authoring. `blueprint new --intent` returns the exact-intent scaffold and three compact scoped recall cards (`--limit`: 1..20). JSON includes `scaffold`, `yaml`, `cards`, `recall` and `unavailable`. Cards expose stable `mem:`/`catalog:` IDs, hashes, provenance, scores, WHY, port schemas and node counts, never executable bodies; open one with `blueprint show <card-id>`. Ranked recall uses current intent vectors, descriptions and enrichment for saved solutions and compatible built-ins after Team/workspace scoping. Missing vectors report degraded/pending status without lexical fallback; incompatible catalogue contracts report unavailable.

Reuse a fitting card by a typed `call`; require the complete purpose and port contract to fit. Scores alone do not prove fit; author from scratch when none fits. Call checking returns the sealed native definition, including its `{id, hash}` pins and immutable dependencies; save that returned definition before apply. Missing revisions refuse and retained pins never silently rebind.

Literal call arguments must satisfy the complete callee schema. Each array item is checked independently, allowing differing values and optional fields; empty arrays satisfy `items`. Required fields, enums, `maxLength` and `additionalProperties` apply to each item. `catalog:checks-fan-in` accepts mixed pass/fail objects or `checks: []`.

## Saved solutions

```bash
domios blueprint keep <id> --when "Watch CI and report failing jobs early" --tag ci --tag github --when "tell me when a pull request goes red" --when "ping me once a check fails"
domios blueprint apply memory:<recipe-id> --set key=value
```

Only the Team Orchestrator keeps solutions. Supply 1–5 intents, preferably 3–5 distinct requests. Generalization retains required typed inputs and removes instance defaults. Recall preserves server ranking, per-intent vectors, semantic canon, structural signatures, enrichment provenance and bounded variant merging. Saves retain their own entry and report cap-archived IDs; unavailable workers leave enrichment pending.

Callable names must describe their contents. Keeping different contents under an existing name in the same scope refuses with `blueprint_name_conflict` and suggests a purpose-based name. Revise an existing solution by ID. Automatic callable merges require matching executable contents and agreeing purposes; distant purpose vectors never merge. Without purpose vectors, only identical purpose text can merge. Presentation-only title and note changes do not change executable identity. Retained pins continue to use their immutable revisions.

Recipe purposes/inputs: `domios docs BLUEPRINT-RECIPES`.

A callable declares a schema object in `result.type`, for example `result: {type: {type: integer}, from: add.out}`; bare type names refuse.

Save a triggerless typed callable without scheduling it:

```bash
domios blueprint keep --file increment.yaml --when "Increment a typed counter" --tag data-transform
domios blueprint show mem:<solution-id> --json
```

Keep retains typed inputs and returns a stable `mem:` ID/hash for `call.ref` and structural `lit`/`input`/`bind` arguments. Imports resolve relative to each author file; absolute paths, traversal, symlinks and junctions refuse with `contract_violation`. The CLI sends bytes; the host resolves scoped nested calls. Save/apply/import/update freeze the transitive closure; export preserves pins. Archived heads refuse new pins, existing pins never rebind, and missing pinned bytes refuse execution. Edits, merges, archives and eviction expose reasons and semantic changes. `keep --file` saves callables; `apply` schedules graphs. `show` exposes pinned bodies, bounded nested results, stops and call stacks; unavailable historical definitions are explicit. Host call checks share apply admission, preserve input signatures and schedule nothing. `check --fix` refuses calls.

Callable `other` handlers are refused at check time; move failure handling to the caller. Inside callees, notify/send/cron/escalate use caller scope and the shared executor claim. Blueprint activation, stop and prompt-response effects require scheduler custody and are refused at check time inside calls; put those effects in the caller.

The simulation harness is internal and has no CLI verb. It records bounded delivery provenance in memory with zero delivery and no real run journal. Process, code, agent, HTTP and conversion nodes refuse before launch; pure filters and calls share the normal budget and port checks.

Restored PTY results retain the exact saved provider/session in a durable outbox for its first authenticated launch, with a fixed 24-hour TTL. Held results keep their run ID/timestamp; live bodies stay unchanged. Closed recipients, changed provider/session, failed launch or expiry create an idempotent delivery-ID failure receipt in the producer's AgentPad, a status event and, for live producers, a failure notification. Terminal causes survive crashes. Interrupted settlement re-emits status by delivery ID; edited receipts are preserved without blocking other results. Possibly written results stay uncertain and never replay. Restored chat backends use the existing reopen path and report reopen failures; this durable hold does not yet cover them.

R10 and environment scrubbing prevent accidents, not hostile code. R10 joins split literals and refuses whole words `domios`/`codeterm`, tokens prefixed `DOMIOS_`/`CODETERM_` with `ambient_authority`; domios-cli capabilities also refuse. Obfuscation can pass. Children scrub instance credentials/private CLI paths and shadow CLI names with private PATH-first deny stubs, retaining shared paths such as `/usr/bin`. HOME/APPDATA and absolute executables remain available under user authority. Unavailable R13 comparisons never claim PASS.

<!-- revision: 74 -->
