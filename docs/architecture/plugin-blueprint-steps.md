# Plugin Blueprint steps

Plugins declare a set of vendor-neutral operations through
`capabilities.graphNodes.steps`. Specialized nodes and general schema-driven
nodes coexist. The host dispatches only declared exports and revisions.
`blueprint nodes` discovers them; installed validation gates YAML admission.
This source targets the next release. A merged SHA and published minimum-host
version must be reported separately; authored fixtures are not runtime proof.

## Descriptor and forward dataflow

`contractVersion: 1` versions the protocol; `revision` pins each descriptor.
Namespaced ids, titles, params, named inputs/outputs, declared outcomes,
`lifecycle: {reconcile,cancel}` and `effects: {sideEffects,potentiallyPaid}`
are bounded. A plugin declares 1..32 nodes, at most16 ports and16 items/port.
Optional inputs use `minItems: 0`; bounded arrays permit reference collections.

Parameters and JSON ports use the closed `PluginSchema` subset: nested closed
objects (32 properties), bounded arrays (256 items), scalars, nullable types,
and finite discriminated `oneOf` alternatives. Schema/value size is64KiB,
depth8, nodes256. References, arbitrary combinators, extra keywords and
unbounded/opaque JSON are refused. Model ids remain plugin-owned strings,
validated against the plugin's current catalog by the adapter; core contains
no supplier catalogs. Converter/event schemas keep their existing contracts.

YAML uses `uses: fixture.generate`, pinned `plugin.revision`, spending unit,
run ceiling/step allowance, `with:` params and named `bind:` edges. Bindings
reference earlier output ports. Routing is forward-only; retries are explicit
unrolled branches. Runtime values carry text/JSON or host run-media references.
Raw paths, frame tokens and executable model output are not edge authority.
Generic file media is currently refused at installed admission; confined
passive image/audio/video delivery is supported by the resolver.

## Durable operations and owner decisions

The host stamps workspace/Team/run/node/attempt/owner, a stable operation id,
run/node labels, opaque run-view reference and one-hop producer lineage.
Labels are frozen by the host. Run ordinals are allocated when a plugin run
first reaches this layer; they do not retroactively enumerate legacy runs.

The transactional ledger writes `prepared`, then `submitting` plus reservation
before submit. A valid first job id commits independently of later payload
validation. Error, removal, cancellation or restart uncertainty becomes
`unknown`, never a paid retry. Unknown permits only exact-job reconciliation.
Shared canonical-store leases serialize submit/reconcile/cancel. Concurrent
cancel records intent until the exact job can confirm a terminal result.
Cancel/discard retains earlier outputs and conservatively retained spend.

States are `prepared`, `submitting`, `queued`, `running`, `succeeded`, `failed`,
`canceled`, `unknown`, `waiting_for_owner_ok`. Plugin holds are persisted beside
the exact runner cursor/outputs and remain independent of `PlanLimitLocked`.
Missing dependency/revision/input is a visible hold, never substitution.

Each exact atomic unit has a separate immutable run ceiling; unlike units are
never added or converted. Amounts are safe nonnegative integers. Missing,
stale/unsafe quotes or exceeded allowances hold before paid admission.
`refresh_estimate` invokes only estimate, never reserves or submits. A safe,
fresh quote becomes an `owner_confirmation` hold; there is no unbounded
unknown-price approval. A decision key binds the current quote/question and
immutable operation. Resume re-estimates before reserving. Owner approval is
an explicit exception up to that operation's unit/maximum; it does not raise
the ceiling or authorize any later operation.

Owner notifications and a private-document decision route use the same native
handler. `ct.decideOperation({operation,decisionKey,action})` accepts only
`approve`, `check_price`, `check`, `cancel`. Approval first previews the host
quote and opens a host-owned view bar; dismissal records nothing. The bar and
`ct.openRun(reference)` require app-observed foreground gesture, a shared
per-frame gate and a current document generation. Navigation opens the actual
run inspector. Document headers carry the private lease/registration, never
iframe authority. Retirement may suppress a reply; persisted receipts remain.
Spending answers require exact Local identity at the HTTP boundary. Generic
tab, peer, service-push and channel notification answers cannot assert owner
authority; ordinary non-spending notification answers keep their existing path.

Static `capabilities.costUnits` maps at most32 atomic unit tokens to
`{one,other,currency?:{code,scale}}`. Names are trimmed ASCII labels <=48bytes;
code is three uppercase ASCII letters and scale0..15. `CostUnits::format` and
SDK `formatCostAmount` share an exact-decimal fixture. No rounding/conversion:
trailing fractional zeros are dropped beyond two places when scale>=2.
Undeclared units render explicitly as `5 units (unit_name)`. Studios consume
this same formatter/declaration; iframe formatting never authorizes spend.

## Frame-independent exact-job observation

A view plugin declares `capabilities.jobLifecycle:{contractVersion:1,
reconcile:true,cancel:boolean}` and implements `jobReconcile`/`jobCancel`.
`host.jobs.admit()` stores host-derived owner/workspace/Team/origin before an
adapter effect. It authorizes observation only, not submission or spend.
`host.jobs.track(operation,job)` retains the first exact known job before
monitor admission, even if the originating document has retired. Its opaque
operation receipt is backend-only. Replacement jobs refuse. `status`, `check`,
`cancel`, `stop` and `operations` require the current verified owner scope.

Agent surface calls use a separate Rust-only projection of the verified
logical handler, not a fabricated document context. Projection spans sanctioned
continuations and clears on final/error/unwind/cancel. Continuation keys must
match a registered plugin-owned job and the current root/logical invocation;
foreign, stale or consumed markers refuse before consuming a receipt.
Root entry snapshots the registration only when the registered view belongs
to that actual VM runtime. A retained old VM cannot borrow a replacement's
registration, including replacement before it requests a surface ticket.
`return host.surface.dispatch(verb,args)` carries the synchronous continuation
bridge through `onAgentCommand`; it never returns a JavaScript Promise.

One persisted monitor stores exact scope/origin/job, next check, deadline,
cancel intent and claims. Bounds:1024 total/256 per plugin,4 workers/1 per
plugin,1s..15min interval,30s checks,7days/4096 checks. It invokes reconcile
(or supported best-effort cancel) only, persists state, then invalidates current
opted-in views by host owner scope. It is not another workflow engine. Close
of a frame does not stop admitted work; owner/workspace/Team loss, removal,
explicit stop and deadlines stop observation without inventing cancellation.
Native blocking functions/kernel I/O cannot be forcibly preempted; retained
process leases exclude overlapping callbacks and durable claims fence results.
Blueprint Stop persists exact-job cancellation intent and dispatches supported
best-effort cancellation at most once. Uncertain cancellation stays unknown;
restart never repeats the cancel effect. Nonterminal and unavailable states
update the matching saved hold before view invalidation. Owner-scope loss
stops observation without creating cancellation authority. Studio `stop`
remains observation teardown, distinct from `cancel`.
Status callbacks still run within existing capability grants plus bounded
physical requests; the callback contract is not a new general sandbox.

## Durable media and agent review

Outputs identify existing plugin artifacts with `{kind:"media_source",id,media}`.
`RunMediaFiles` invokes the default-refusing `run_media_file` port with a
Serialize-only host `RunMediaLookupContext`, an effects-denying observer and
five-second/narrower deadline. It never fabricates a frame. Producer identity
is fenced before/after; confined retained files are read at fixed length and
hashed. The registry persists scope/owner/producer operation+port/id/fingerprint
and receiving-node grants, never an open path/frame token. Disappeared or
changed bytes refuse. A verified submitting receiver can use the existing
`host.transfer.bindInput` continuation to mint a fresh one-use destination/
runtime-bound selected-input grant. Restart re-resolves the same fingerprint;
no spent grant is reused.

Agent steps receive actual `AgentTaskInput.media` bytes/MIME/SHA/lineage only
when their backend declares the modality. Limits:4 media,2MiB each/8MiB total.
Text-only backends refuse media. `review:true` returns a bounded closed
`{verdict:"pass"|"retry"|"choose",notes?,choice?}`; verdicts route the forward
graph. A text artifact reference is never claimed as pixel/audio perception.

The no-paid fixture `tests/fixtures/blueprint-plugin-media-review.yaml` is run
by `pipeline::plugin_tests`: a real PNG reaches a fixture JS agent,3 plugin
jobs include1 failure, and a fourth operation waits over its run ceiling.
Separate fixtures approve/revalidate/resume the exact cursor, cancel without
losing outputs, mint fresh media inputs, recover known job receipts after
frame retirement, and sabotage stale/foreign continuation authority.
Exact canonical commands and unrun/actual results accompany the commit report.

Manifest, loader sandbox construction, VM entry and capability registration
share strict bounded plugin ids: lower-case letters/digits/hyphen/underscore,
an alphanumeric first character, and no Windows device names. Paths, dotted
host SQLite filenames and mismatched runtime identities refuse before entry.
This closes the new ledger-file collision. Existing host directory namespace
collisions (a valid id naming a pre-existing host subdirectory) require a
separate data-root design; this change does not claim to eliminate them.
