# Runtime event path

One event path for every runtime provider. The host owns a normalized `RuntimeEvent` and the per-tab maps it updates. A plugin maps a drained batch of transport frames onto those events. Core selects the declared transport capability and never a provider identity.

Chat, delivery, and session binding stay under the [core essence](core.md) and the [agent provider capability](capabilities.md#agent-models). This contract owns batching, the event model, the budgets, the failure policy, and the transport set. Where core describes the agent runtime as stdio, that is today's shipped transport; growth adds a transport value under this contract and does not add a provider branch.

The host keeps reducer state for the life of the tab's runtime epoch. A frame never causes a full-state serialization round-trip. Batching drains frames that are already queued. It does not wait for a later frame, and it does not use a time window.

```map
anchors:
  - core:agents-and-providers
  - core:plugin-host
  - core:pty
  - capability:agent-providers
  - capability:message-delivery
  - capability:session-continuity
  - capability:usage-and-analytics
```

## RuntimeEvent

An event is an upsert. A later event for the same identity replaces the earlier snapshot. Events carry no provider identity.

| Event | Identity | What it updates |
|---|---|---|
| `SessionUpdated` | the tab's runtime session | Auth, login, and billing mode |
| `ThreadUpdated` | `threadId` | Parent thread, settings, and thread status |
| `TurnUpdated` | `threadId` + `turnId` | `working`, `idle`, or `needs_input`, with detail, status line, parent thread, and start or completion time |
| `ItemUpdated` | `threadId` + `itemId` | One turn-item snapshot |
| `RequestUpdated` | `requestId` | A server request or question: pending, resolved, or cancelled |
| `EvidenceUpdated` | `sendId` | The thread and turn that match one claimed send |
| `UsageUpdated` | `threadId` | Token usage |
| `RateLimitsUpdated` | `threadId` or the session | Quota windows |
| `CompactionUpdated` | `threadId` + `itemId` | A completed compaction marker |
| `Warning` | one bookkeeping failure | The single visible warning for that failure |

`TurnUpdated` with `idle` is the turn terminal. `detail` is `interrupted`, `failed`, or absent when the turn completed. A missing `turnId` on `needs_input` is the thread waiting on a request whose turn is the active one.

### Turn item

The item map stores a full snapshot. Unknown item kinds are dropped from the projection and do not fail the batch.

| Kind | Projects to the transcript as |
|---|---|
| `user_message` | the user row, and `EvidenceUpdated` only when the text and attachments match the claimed send |
| `assistant_message` | assistant text |
| `reasoning` | a collapsed thinking row |
| `plan` | assistant or plan text |
| `command` | a command tool row |
| `file_change` | an edit tool row |
| `tool_call` | a generic tool row, including MCP and dynamic tools |
| `question` | a question request, together with `RequestUpdated` |
| `approval` | an approval request, together with `RequestUpdated` |
| `compaction` | the compaction marker |
| `subagent` | a child thread link, together with `ThreadUpdated` |
| `notice`, `error` | a non-fatal row or `Warning` |

`file_search` and `web_search` stay in the map. They gain a transcript row only when the transcript already has a row type for them.

### Per-tab maps

The daemon holds these maps for one runtime epoch and one binding:

- items, keyed by thread and item
- turns, keyed by thread and turn
- threads, keyed by thread, including the parent edge
- requests, keyed by request id
- evidence, keyed by send id, consumed once
- usage and rate limits, latest per thread
- session, the latest auth and login snapshot

An epoch change drops the maps. Downstream readers use the maps and the events, not a plugin state blob:

- tab live state reads `TurnUpdated` and `SessionUpdated`
- the transcript reads `ItemUpdated` through the existing reconcile
- delivery evidence reads `EvidenceUpdated` and confirms only an exact claimed send on the bound thread, nonce, and epoch

History reconcile updates the maps. It does not confirm a send and it does not replay a failed batch.

## Batch mapping

```
mapFrames(context, frames, scratch) -> { events, scratch, faults }
```

`frames` are the frames already drained, in arrival order. `events` follow that order. One frame may emit nothing, or several events. `faults` name a frame index and a reason. The mapper continues with the next frame. On a fault, `scratch` stays at the value from before that frame.

`scratch` is opaque JSON. The host stores it and passes it back on the next call. The host does not interpret it. It exists for mapping state that is not yet an item snapshot: a partial reasoning accumulation, and a subagent parent edge before `ThreadUpdated` is emitted. Item maps are not copied into scratch.

### Parity oracle

The same fixture of frames and starting scratch is applied to the native mapper and the QuickJS mapper. The oracle requires equal events and equal scratch. Fixture tokens are opaque. The oracle does not compare prompt wording. The Codex mapper slice adds this case on the `mapFrames` hook. Until that slice, the existing `runtimeReduce` parity remains the oracle for the hook that is actually called.

### Drain

A batch is every frame already queued, capped at 1024, taken with a non-blocking receive. The subscriber waits only when the queue is empty, and then only for the next frame, process end, or stream end. That wait is the idle wake. After the first frame, further frames join the batch only when they are already queued. The next batch starts immediately when more frames remain. No timer, sleep, or coalescing window decides a boundary.

Server requests keep arrival order. Each request in the batch is registered before the single hook call. After that call, unmapped requests receive their protocol error in the same order. A later request is not answered before an earlier one. A request is not held to see if another frame arrives.

### First slice

The first implementation keeps `runtimeReduce` and calls it once per drained batch. The host translates each returned agent event into the `RuntimeEvent` values above and folds them into the maps. Live state, the transcript, and delivery evidence read those events. The plugin state blob still returns once per batch, not once per frame. The Codex mapper slice replaces that blob with `mapFrames` and scratch. This contract forbids a per-frame full-state round-trip in both slices.

The translation is one-to-one with the agent events the transcript, live state, and evidence already consume, so those surfaces keep their current observable results.

## Budgets

Bounds are per tab and per runtime epoch. Overflow is amortized: the host truncates, retires, or records a bookkeeping failure. It does not multiply state size by the number of frames.

| Budget | Bound | Overflow |
|---|---|---|
| Broadcast queue | at least 8192 frames | A lag is a bookkeeping failure |
| Drain batch | 1024 frames | The remainder is the next batch, with no wait |
| Emitted values | 4 MiB per batch | Bookkeeping failure |
| Plugin state blob, until `mapFrames` | 256 KiB per batch | Bookkeeping failure. A per-row text cap is not a total budget |
| Scratch, once `mapFrames` owns mapping | 64 KiB | Bookkeeping failure. The host keeps the previous scratch |
| Item maps | 1000 live items or 8 MiB of item text, first reached | Heavy text keeps its tail. The oldest closed items retire. The process stays up |
| Outstanding server requests | 64 | The excess request gets an immediate protocol error. The process stays up |

A successful batch sets the consecutive bookkeeping count to zero. dev.6's total plugin-state budget stays in force until `mapFrames` removes the blob.

## Failures

A bookkeeping failure is a frame fault, a failed hook call, a state, value, or scratch overflow, or a lagged broadcast. Each one produces one visible warning on the runtime tab and one resync. The provider process stays up. The turn continues.

Resync runs the reload the host already uses for a bound thread: `ResumeThread` for that thread, then `ReadHistory` until the history page ends. Maps reconcile from those pages. Scratch is unchanged when the whole call fails, and rolled back to the pre-frame value when one frame faults. The failed batch is not applied and not replayed.

One failed hook call is one batch failure and one warning, including when the batch held many frames. A frame fault inside a successful `mapFrames` call is one warning for that frame, and the other frames still apply.

The process closes only when:

- the provider process exits
- the transport fails as a protocol: a frame cannot be decoded, or stdin, stdout, or the event stream breaks
- eight consecutive batches fail bookkeeping with no successful batch between them

The count is per runtime epoch. Close unloads that epoch, keeps a visible reason, and offers Retry. The first bookkeeping failure does not close the process. A lag of the frames the queue could not deliver is a warning and a resync.

## Transport

`agentRuntime.runtime` is a closed enum. Core matches the enum. An unknown value fails manifest validation.

| Value | Bytes | Who declares it |
|---|---|---|
| `stdio-jsonrpc` | Newline-delimited JSON-RPC on stdin and stdout. Frames are notifications and server requests | Codex `app-server`. OpenCode `acp` |
| `http-sse` | The host starts the declared command, passes the bound address through the command's declared arguments, and reads a server-sent event stream | OpenCode `serve` |

The `http-sse` manifest names the stream path and the auth environment variable. The credential is injected from the secret store into that variable. Core does not recognize a provider-specific variable name, port, or URL.

A stream that ends because the process exited is a protocol failure. A stream that ends while the process is alive is a bookkeeping failure: warn, resync from history, and open the stream again only after a new subscription is accepted. There is no timed reconnect. A stream that cannot be resubscribed counts toward the consecutive limit. Which OpenCode serve stream path is declared is the plugin's choice; core does not select a version by provider identity.

## Wire mapping

`mapFrames` performs this mapping. The host does not match these method names. After mapping, a consumer cannot tell which transport produced an `ItemUpdated`.

### Codex, `stdio-jsonrpc`

| Wire | Event |
|---|---|
| `account/updated`, `account/login/completed` | `SessionUpdated` |
| `thread/started`, `thread/status/changed`, `thread/settings/updated`, `model/rerouted` | `ThreadUpdated` |
| `thread/tokenUsage/updated` | `UsageUpdated` |
| `account/rateLimits/updated` | `RateLimitsUpdated` |
| `turn/started`, `turn/completed` | `TurnUpdated` |
| `item/started`, `item/completed`, and item deltas for agent text, plan, reasoning, command output, file change, and tool progress | `ItemUpdated` |
| `thread/compacted` | `CompactionUpdated` |
| permission and question server requests | `RequestUpdated` pending, and `TurnUpdated` `needs_input` |
| `serverRequest/resolved` | `RequestUpdated` resolved |
| a user-message item that matches a claimed send | `EvidenceUpdated` |
| an `error` notification that is still valid JSON-RPC | `Warning` or an `error` item. The process stays up |

Parent links and reasoning text may sit in scratch until the snapshot event is emitted.

### OpenCode serve, `http-sse`

Two stream shapes produce the same events.

Event subscribe:

| Wire | Event |
|---|---|
| `message.updated`, `message.part.updated`, `message.part.delta` | `ItemUpdated` |
| `session.created`, `session.updated`, `session.status` | `ThreadUpdated` |
| `session.status` busy, `session.idle` | `TurnUpdated` |
| `session.compacted` | `CompactionUpdated` |
| `permission.asked`, `question.asked` | `RequestUpdated` pending |
| `permission.replied`, `question.replied`, `question.rejected` | `RequestUpdated` resolved or cancelled |
| `todo.updated` | `ItemUpdated` plan |
| `session.error` | `TurnUpdated` idle `failed`, or `Warning` |

`/event`:

| Wire | Event |
|---|---|
| `session.text.*`, `session.reasoning.*` | `ItemUpdated` assistant or reasoning |
| `session.tool.*` | `ItemUpdated` command or tool |
| `session.compaction.*` | `CompactionUpdated` |
| `session.execution.started` | `TurnUpdated` working |
| `session.execution.succeeded`, `interrupted`, `failed` | `TurnUpdated` idle with the matching detail |
| `session.created` with a parent | `ThreadUpdated` |
| `permission.asked`, `form.created` | `RequestUpdated` pending |
| `permission.replied`, `form.replied`, `form.cancelled` | `RequestUpdated` resolved or cancelled |

A client-minted message id can make a prompt retry idempotent. The host does not mint it, and a mapping failure does not retry the prompt.

### OpenCode acp, `stdio-jsonrpc`

`opencode acp` speaks Agent Client Protocol version 1 as newline-delimited JSON-RPC on stdio. The same events:

| Wire | Event |
|---|---|
| `session/update` `agent_message_chunk`, `user_message_chunk` | `ItemUpdated`. `user_message_chunk` emits `EvidenceUpdated` only when it matches a claimed send |
| `session/update` `agent_thought_chunk` | `ItemUpdated` reasoning |
| `session/update` `tool_call`, `tool_call_update` | `ItemUpdated` command or tool. A child session id in the update's meta is `ThreadUpdated` |
| `session/update` `plan` | `ItemUpdated` plan |
| `session/update` `usage_update`, and `usage` on the prompt result | `UsageUpdated` |
| `session/update` `compaction_update`, `compaction_summary_chunk`, or compaction meta on `session_info_update` | `CompactionUpdated` |
| `session/update` `config_option_update` | `ThreadUpdated` settings |
| `session/request_permission`, `elicitation/create` | `RequestUpdated` pending and `TurnUpdated` `needs_input` |
| `session/prompt` completion and `session/cancel` | `TurnUpdated` idle, with `interrupted` when the stop reason is cancelled |

`available_commands_update` does not create a transcript row.

## Slice order

1. This contract.
2. Codex on `runtimeReduce`: one call per drained batch, host translation into `RuntimeEvent`, warn and resync, and `http-sse` added to the transport enum. Codex stays `stdio-jsonrpc`.
3. Codex `mapFrames` replaces the state blob. The parity oracle lands with that hook.
4. The OpenCode transport follows the owner's choice of shape. It consumes this mapping and adds no second event model.

## Designs that fail this contract

- A time window that waits for more frames before reducing.
- Closing the provider process on the first bookkeeping failure, including lag and a reducer size limit.
- A core branch on a provider id.
- An event model that only Codex frames can fill.
- A native adapter per provider that bypasses the plugin boundary.
- Serializing the full reducer state once per frame.
- A timer-based event-stream reconnect.
- Treating a retired sidecar normalizer as this path.
