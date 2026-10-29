# Durable media lookup effects boundary

Durable run media lookup uses a separate host-owned, lookup-only provider port. It never fabricates a document, frame, surface, or view lease. The port remains unavailable until the lookup adapter, verified run-reference resolver and this effects guard have been integrated and reviewed together.

`codeterm_decision::physical::RequestObserver::effects_allowed()` defaults to true. Ordinary plugin calls retain their existing permissions. A durable lookup caller enters `RequestScope::observation_only(deadline)`, which refuses all physical requests, including GET and HEAD, and uses the narrower caller deadline. Nested scopes cannot relax an existing denial. Dropping the scope restores the preceding observer, including during unwinding.

The lookup adapter must refuse missing or permissive observers. It captures the exact observer before dispatching blocking work and reinstalls it around the complete VM invocation and synchronous continuations. Deadline and cancellation are separate requirements; dropping a waiter does not prove arbitrary native work has stopped.

The admission audit covers these native bindings before existing permissions, cached grants or host service references can authorize work:

| Port | Observation behavior |
| --- | --- |
| Subprocess execution and queued execution | Refuse in `PluginSandbox::can_exec` |
| HTTP fetch, queued fetch and streaming | Refuse in `can_reach`; physical request observation also refuses in the transport worker |
| File, atomic, delete, directory and lock writes | Refuse in `can_write` |
| Project writes | Refuse before consulting cached project grants |
| Temporary file creation and explicit unlock | Refuse directly in the native binding |
| Service-backed worker, agent, tab, workspace, surface, event, configuration, secret and recording operations | Service resolution returns no service; recording capability also refuses |
| Exec close and stream close/control | Refuse directly in the native binding |
| Exec poll or `awaitJob` for ordinary effect jobs | Refuse without consuming or canceling an admitted receipt |
| Stream poll | Refuse without consuming its state |

Allowed lookup operations retain their normal confinement and bounds: file reads and metadata, read-only SQLite queries, path/string/hash helpers, clocks, public manifest/settings/credential metadata and diagnostics. Diagnostics must not contain secrets. Read-only SQLite jobs may complete through `awaitJob`; their retained job records are host-created, and their only production producer is the read-only SQLite query port. A plugin cannot relabel an effect job as a read query. Internal read caches and query tickets are ephemeral host state, not paid operation receipts.

The guard does not create filesystem or network grants. It does not sandbox arbitrary native Rust plugin implementations, which remain trusted host code subject to the lookup trait contract. It does not stop effects already admitted by an earlier call. Already-admitted job receipts remain available to their original controller, and neither lookup cancellation nor frame closure implies provider cancellation.

The binary transfer layer independently refuses start and input binding in its native durable-lookup scope. Trusted view context remains absent throughout that path. Run scope, producer registration, typed handoff and immutable file identity are separate resolver checks; attribution metadata does not grant authority.

Focused executable fixtures are authored for nested scopes and unwinding, observer projection into a worker, actual low-level host binding refusal, a queued refusal, read preservation, populated service and cached project grants, preservation of an admitted effect receipt, and the actual HTTP worker's pre-transport refusal. These are source fixtures until canonical execution reports otherwise. No paid calls are needed.
