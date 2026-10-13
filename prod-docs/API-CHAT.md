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

Local `codeterm send` uses socket `tab.send_text`: `OK (sent)` after a confirmed write, `OK (pending)` when the route accepted background work or wrote bytes without submit confirmation, a refusal when no bytes were written. A substituted send names its recipient inbox file as `pad_file`; human output prints the path and multi-target JSON reports `pad_files`. With `--json`, `delivery` and any `bytes_written` field carry the same distinction for one target; multi-target JSON includes `pending_written` for accepted writes that still lack submit confirmation. The resolved payload echo is emitted for confirmed delivery or a pending write.

`POST .../chat-send` is the Chat UI user turn. A PTY tab without agent identity is refused before the send transaction so prose never reaches a bare shell. Accepted sends open a ledger entry for later reconciliation; `GET .../chat-send` reads it, `state` one of `accepted`, `unknown`, `confirmed`, `failed`.

Local and mesh `codeterm send` open no Chat send ledger entry, so `GET .../chat-send` never lists them.

Neither route promises the recipient *answered*. Replies come through the sender's return-address channel (`codeterm send --tab ID [--mesh PEER]`, or `codeterm agent report` for a Worker). The recipient's visible terminal text is never a delivery signal.

### Transcript and send route index

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/chat-send` | `{text?, client_token?}` (shares the `/send` body type; only these two are read) | Chat UI user turn. Adds a ledger entry to delivery; returns one `ChatSendStatus` (`send_id`, `state`, `ledger_id`, `sequence`, `correlationId?`). `client_token` is echoed verbatim, never interpreted. The CLI does not use this route. |
| GET | `/api/tabs/{id}/chat-send` | — | This tab's chat-send ledger (`ChatSendStatus[]`), including submitted socket user sends admitted by the same owner; states and reconciliation: API-CHAT. |
| GET | `/api/tabs/{id}/chat` | `?since=N` | Stateless transcript read, resolved from the tab's own session record. `{messages, count, pending}`. A tab with no bound session answers `pending: true` with an empty list — never another tab's transcript. Unknown tab → 404 |

#### Delayed Chat messages

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/tabs/{id}/delayed-messages` | — | List queued and failed delayed Chat UI messages for this tab |
| POST | `/api/tabs/{id}/delayed-messages` | `{text, scheduledAtMs, createdByTab?, recurrence?: {everyMs, untilMs?, count?}}` | Queue a one-shot or recurring message for this tab. Recurrence starts at `scheduledAtMs`, repeats at an anchored interval of at least one minute, with at most one stop condition. Normal send is unaffected. |
| PATCH | `/api/delayed-messages/{id}` | `{text?, scheduledAtMs?, recurrence?: {everyMs, untilMs?, count?}}` | Edit a queued or failed delayed message, including its recurrence rule |
| DELETE | `/api/delayed-messages/{id}` | — | Delete a queued or failed delayed message |
| POST | `/api/delayed-messages/{id}/send-now` | — | Deliver a queued or failed delayed message immediately |

#### Chat attachment authority

The desktop `chat_start` command and web-mirror `POST /api/tabs/{id}/chat-raw/start` return the same bounded snapshot shape. `source_session_id` and `session_binding_nonce` are derived from the Tab and form one authority generation; a client adopts snapshots and `tab-chat-msg-{id}` deltas only when both match the Tab's current generation.

Snapshots and backfill return `offset_kind`: `bytes` identifies complete-record byte windows; `items` identifies structured item paging positions. Paging positions never identify rows or imply byte gaps. Message `id` remains the provider item or source-record identity across snapshots, live deltas and backfill; equal content under different ids remains distinct.

`pending=true` without session authority means the Tab awaits its own session and never falls back to another; with authority and a subscription it awaits its own store. `pending=false` is a complete bounded snapshot, possibly empty. Visible Chat owns one subscription and takes a fresh snapshot on reactivation or resync.

#### Interactive chat prompts

Chat messages may carry an `interactive` array of answerable prompt widgets parsed from the provider's session stream by providers that declare the capability. It rides both chat wire shapes: the `GET /chat` and `chat/start` snapshot (`RichMessage[]`) and live `tab-chat-msg-{tabId}` events (`ChatMsg[]`, on the tool-call message). First element type:

```json
{ "type": "question", "id": "toolu_42-0", "toolUseId": "toolu_42",
  "prompt": "Which database?", "header": "Database",
  "options": [{ "label": "Postgres", "description": "Relational" }], "multiSelect": false }
```

The enum is open for future element types. The frontend renders each as a clickable widget and routes the answer through the tab send path (`/api/tabs/{id}/send`).

<!-- revision: 1 -->
