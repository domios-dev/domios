# Plugin Blueprint step foundation

This foundation discovers plugin step declarations and provides an isolated,
fixture-tested execution port. Blueprint runner admission remains closed:
`blueprint nodes` returns `available: false` and
`hold_reason: plugin_step_runner_unavailable` for these descriptors. This is
not a shipped claim of runnable plugin pipelines. The release target is 1.18;
no minimum host release is published by this source change.

## Declaration and adapter

`capabilities.graphNodes.steps` declares 1–32 descriptors. IDs belong to the
declaring plugin's namespace, such as `fixture.generate`. A plugin can expose
both specialized operations and a general operation with schema-driven model
and parameter choices. `contractVersion: 1` versions the host protocol;
`revision` pins an individual descriptor's parameter and port contract.

Each descriptor carries title, description, params, named inputs/outputs,
outcomes, lifecycle `{reconcile, cancel}`, and effects
`{sideEffects, potentiallyPaid}`. Parameters and JSON ports reuse the closed
NodeSchema object/scalar/bounded-scalar-array subset. This is a restricted
JSON Schema 2020-12-shaped vocabulary, not a full JSON Schema implementation.
References, combinators and nested objects are refused. Each descriptor and
port payload is bounded to 64 KiB, with at most 16 ports and 16 items per port.
`minItems: 0` permits optional inputs. JSON ports require a schema.

JS exports are `blueprintEstimate`, `blueprintSubmit`, `blueprintReconcile`,
and `blueprintCancel`. Registration requires submission and every declared
paid/lifecycle export. Native implementations use `BlueprintStep`. Requests
carry a trusted context, descriptor ID/revision, params, typed inputs, unit,
step allowance, persisted job ID and bounded deadline. QuickJS calls use the
existing interruptible cancellation/deadline mechanism.

## Operations and cost

The host supplies workspace, Team, run, node, attempt, owner and operation
identity, plus run/node labels, run-view attribution and one-hop input lineage.
Runner stamping and a real run navigation target are follow-up integration;
the foundation accepts these only from its trusted Rust caller.

The SQLite operation ledger commits `prepared` and then `submitting` plus a
reservation before the adapter can submit. Every later attempt must match the
persisted request, except for attempt identity. A submission error, interrupted
submission or recovered `submitting` state becomes `unknown`. It never invokes
submission again. Only reconciliation can recover a job; a persisted job ID
cannot be replaced. No callback transport is introduced here.

`prepared`, `submitting`, `queued`, `running`, `succeeded`, `failed`, `canceled`,
`unknown`, and `waiting_for_owner_ok` are distinct states. Missing/stale estimates
and exhausted step/run allowances hold with a closed waiting reason and numeric
estimate/remaining/unit fields. A recorded owner approval applies to the exact
immutable operation and maximum amount. Resume re-estimates before reserving.
The approval cannot authorize another operation. Estimates never authorize spend.

Amounts are integers from zero through JavaScript's maximum safe integer.
Use an explicit atomic unit (`usd_micro`, `credit_milli`, `characters`) to avoid
rounding fractional prices to zero. Run ceilings and reservations are independent
per exact unit. The host performs no exchange-rate or cross-unit arithmetic.
Started, canceled and uncertain effects retain their reservation conservatively.
There is no settlement/refund or deletion operation in this foundation.

## Media and sandbox audit

Backend outputs identify existing plugin-owned media with
`{kind: media_source, id, media}`. A trusted `RunMediaResolver` validates and
fingerprints the source; adapters cannot invent a host handle. The host stamps
the producer plugin from its persisted operation, stores an opaque reference,
and checks Team/workspace/run, receiving plugin/node grant, media kind and source
fingerprint on every use. An absent or changed source refuses handoff.
References and grants persist across restart. Repeated publication is idempotent.

This authority layer stores no raw path, file bytes, base64 or frame token.
UI delivery tokens remain frame-scoped. Integration with the confined media
open helper, authorized input delivery, and media-aware agent adapters remains
in the runner batch. The fixture review acknowledges metadata only; it makes
no claim to inspect pixels.

No QuickJS host function, filesystem grant, network grant or secret bucket is
added. Raw adapter errors never enter operation history. Parameter and output
payloads are still plugin content: callers must keep credentials in existing
secret buckets, and runtime history/export redaction remains a runner integration
requirement. Registry snapshots are revoked on removal/reload; an in-flight
call crossing removal is treated as uncertain, rather than replayed.

## Deferred integration

The next batch owns YAML namespaced steps and installed-revision checks,
forward-only typed port bindings, plugin holds/resume beyond PlanLimitLocked,
authenticated owner decision transport, plugin history synchronization, real
run navigation, media resolver wiring and agent media/metadata capabilities.
Until those admission boundaries exist, discovery is informational only.

Focused fixture selectors are `test(blueprint_step)` in `codeterm-plugin` and
`test(plugin_steps_catalog_is_discoverable_removed_and_execution_stays_closed)`
in `codeterm-sentinel-host`. Tests require no paid service, network, shell
execution, live agent or timer coordination. They are authored here; canonical
execution and platform results must be reported separately.
