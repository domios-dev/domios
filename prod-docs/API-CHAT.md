# Chat API

REST contracts for chat sends, admissions, transcripts, snapshots, and delayed messages.

### Text input routes: `/send` vs `/chat-send`

Both routes exist as distinct handlers; choosing is route selection.

| Route | Returns |
|-------|---------|
| `POST /api/tabs/{id}/send` | `{ok:true, delivery:"sent"}` after confirmed delivery, `{ok:true, delivery:"pending"}` when a user send or an enveloped/programmatic agent send is accepted by its background queue, or a structured delivery error with its `bytes_written` status. User sends also return `send_id`. Long limited-provider sends add `pad_file`; batches add `pad_files`. |
| `POST /api/tabs/{id}/chat-send` | One `ChatSendStatus` — `send_id`, `state`, `ledger_id`, `sequence`, optional `correlationId`, and optional `pad_file`; accepts an agent tab at any launch stage or a chat-backend tab; a shell-only tab gets `tab_busy_refused`, `bytes_written:false` |
| `GET /api/tabs/{id}/chat-send` | `ChatSendStatus[]` for the tab, reconciled against session-turn evidence on read |
| `POST /api/tabs/{id}/chat-send/admissions` | Read-only lookup: `{clientTokens:[...]}` returns `{correlationId, evidence}[]`, evidence `present`, `absent` or `unknown`; incomplete retention never proves absence |

`POST .../send` carries user text, mesh relays, service push-token writes, and low-level `actions` / `raw` / `batch` input. User text sent while a provider dialog is classified waits in the durable user-send queue until a PTY state change makes delivery safe; agent and programmatic text use the delivery outbox the same way. A failure serializes the delivery taxonomy (`tab_not_reachable_on_peer`, `tab_busy_refused`, `pty_unavailable`, `submit_unconfirmed`).

Local `domios send` uses socket `tab.send_text`: `OK (sent)` after a confirmed write, `OK (pending)` when the route accepted background work or wrote bytes without submit confirmation, a refusal when no bytes were written. A substituted send names its recipient inbox file as `pad_file`; human output prints the path and multi-target JSON reports `pad_files`. With `--json`, `delivery` and any `bytes_written` field carry the same distinction for one target; multi-target JSON includes `pending_written` for accepted writes that still lack submit confirmation. The resolved payload echo is emitted for confirmed delivery or a pending write.

`POST .../chat-send` is the Chat UI user turn. A PTY tab without agent identity is refused before the send transaction so prose never reaches a bare shell. Accepted sends open a ledger entry for later reconciliation; `GET .../chat-send` reads it, `state` one of `accepted`, `unknown`, `confirmed`, `failed`.

Runtime-tab sends retain a Chat send ledger entry. A possible handoff returns
`delivery:"pending"`, `bytes_written:true`, and `send_id`; this is uncertainty,
not permission to replay. `GET .../chat-send` reconciles the receipt against exact
own-session evidence. Other programmatic transports do not open this ledger entry.

Neither route promises the recipient *answered*. Replies come through the sender's return-address channel (`domios send --tab ID [--mesh PEER]`, or `domios agent report` for a Worker). The recipient's visible terminal text is never a delivery signal.

### Transcript and send route index

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/chat-send` | `{text?, client_token?}` (shares the `/send` body type; only these two are read) | Chat UI user turn. Adds a ledger entry to delivery; returns one `ChatSendStatus` (`send_id`, `state`, `ledger_id`, `sequence`, `correlationId?`). `client_token` is echoed verbatim, never interpreted. The CLI does not use this route. |
| GET | `/api/tabs/{id}/chat-send` | — | This tab's chat-send ledger (`ChatSendStatus[]`), including submitted socket user sends admitted by the same owner; states and reconciliation: API-CHAT. |
| GET | `/api/tabs/{id}/chat-send/dismissals` | — | Durable dismissal receipts `{send_id, send}[]`. Clients keep hidden send correlations to consume later echoes without restoring dismissed rows. |
| POST | `/api/tabs/{id}/chat-send/dismissals` | `{send_id, send:{id, correlationId, content, sentAtMs, echo?}}` | Delete a retained undelivered user bubble. Cancels queued input atomically against claim; a claimed or written send is only dismissed. Returns the durable receipt. Delivered and system sends are refused; never replays input. |
| GET | `/api/tabs/{id}/chat` | `?since=N` | Stateless transcript read, resolved from the tab's own session record. `{messages, count, pending}`. A tab with no bound session answers `pending: true` with an empty list — never another tab's transcript. Unknown tab → 404 |

Sleeping tabs read their own persisted transcript without waking or starting a runtime. A runtime tab awaiting its first session returns `200` with `pending: true` and `state: "waking"`; opening or addressing the tab owns waking it. Snapshot failures return JSON `{code, state, error}` with the original reason in `error`; unknown failures use HTTP 500, code `chat_snapshot_failed` and state `failed`. Request refusals preserve their HTTP status with code `chat_snapshot_refused` and state `refused`.

#### Delayed Chat messages

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/tabs/{id}/delayed-messages` | — | List queued, stashed and failed messages for this tab. Sending messages and system heartbeat projections are excluded. |
| GET | `/api/tabs/{id}/system-heartbeats` | — | Read-only system heartbeat projections, including cadence, reason and delivery ticks. These are separate from delayed messages. |
| POST | `/api/tabs/{id}/delayed-messages` | `{text, scheduledAtMs?, createdByTab?, recurrence?: {everyMs, untilMs?, count?}}` | Past times and times up to one second ahead deliver immediately without entering the queue; later times queue a message. Omit `scheduledAtMs` to stash. Recurrence starts at `scheduledAtMs`, repeats at an anchored interval of at least one minute, with at most one stop condition. After an immediate recurring delivery, only its future occurrence is queued. |
| PATCH | `/api/delayed-messages/{id}` | `{text?, scheduledAtMs?, recurrence?: {everyMs, untilMs?, count?}}` | Edit a queued or failed delayed message, including its recurrence rule |
| DELETE | `/api/delayed-messages/{id}` | — | Delete a queued or failed delayed message |
| POST | `/api/delayed-messages/{id}/send-now` | — | Deliver a queued or failed delayed message immediately |

#### Chat attachment authority

The desktop `chat_start` command and web-mirror `POST /api/tabs/{id}/chat-raw/start` return the same bounded snapshot shape. `source_session_id` and `session_binding_nonce` are derived from the Tab and form one authority generation; a client adopts snapshots and `tab-chat-msg-{id}` deltas only when both match the Tab's current generation.

The start request accepts optional `knownWindow`: `{source_session_id, session_binding_nonce, end_offset, file_length, cursor}` from the client's last adopted byte window. JSONL responses include `file_length` and `kind`: `unchanged` keeps the cached rows with no messages, `delta` appends the returned byte range, and `snapshot` reconciles a bounded snapshot. Authority or file-incarnation mismatch, truncation, an invalid cursor, or growth at least the snapshot byte limit requires a snapshot. Omit `knownWindow` for a cold attach.

Snapshots and backfill return `offset_kind`: `bytes` identifies complete-record byte windows; `items` identifies structured item paging positions. Paging positions never identify rows or imply byte gaps. Message `id` remains the provider item or source-record identity across snapshots, live deltas and backfill; equal content under different ids remains distinct.

`pending=true` without session authority means the Tab awaits its own session and never falls back to another; with authority and a subscription it awaits its own store. `pending=false` completes attachment, possibly with an empty snapshot. Once activated, a retained mounted Chat keeps its listener and subscription while hidden; eviction releases them. A cached remount attaches from its known window, and resync recovers from the adopted cursor or requests a snapshot.

#### Interactive chat prompts

Chat messages may carry an `interactive` array of answerable prompt widgets parsed from the provider's session stream by providers that declare the capability. It rides both chat wire shapes: the `GET /chat` and `chat/start` snapshot (`RichMessage[]`) and live `tab-chat-msg-{tabId}` events (`ChatMsg[]`, on the tool-call message). First element type:

```json
{ "type": "question", "id": "toolu_42-0", "toolUseId": "toolu_42",
  "prompt": "Which database?", "header": "Database",
  "options": [{ "label": "Postgres", "description": "Relational" }], "multiSelect": false }
```

The enum is open for future element types. The frontend renders each as a clickable widget and routes the answer through the tab send path (`/api/tabs/{id}/send`).

Backfill accepts `offset_kind` (`bytes` or `items`) alongside `before_offset`; preserve the snapshot's unit across runtime startup. Providers declaring native stored runtime history can serve recorded messages before their runtime starts; live updates reconcile by the same native item ids.

#### History gaps

A transcript read that starts after its source's first record begins with a gap row: `type: "system"`, `systemSubtype: "history_gap"`, `timelineMarker: {kind: "history_gap", source, session_id, before_offset}` with `source` `provider_session` (an archived context epoch) or `chat_backend` (a chat-backend log). `POST /api/tabs/{id}/chat-raw/history` with `{source, session_id, before_offset}` returns `{messages}`: the older page ending at that offset, led by a new gap row while older records remain. The session must be the Tab's own chat-backend session or one of its archived epochs; any other session is refused with 409.


## Additional REST routes

Chat requests use the target tab's owned session. Bridge thread changes require explicit selection and caller authority.

| Method | Path | Operation |
|--------|------|-----------|
| DELETE | `/api/tabs/{id}/bridge/thread` | Close its bridge thread. |
| GET | `/api/tabs/{id}/bridge/thread` | Read the selected tab's bridge-thread state. |
| POST | `/api/tabs/{id}/bridge/thread` | Open its bridge thread with explicit selection authority. |
| POST | `/api/tabs/{id}/chat-raw/history` | Read paginated raw chat history. |
| POST | `/api/tabs/{id}/chat-raw/resume` | Resume raw chat streaming. |
| POST | `/api/tabs/{id}/chat-send/admissions` | Read admission results for submitted chat sends. |

<!-- revision: 10 -->
