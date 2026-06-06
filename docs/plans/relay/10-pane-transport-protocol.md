# 10 — Pane transport protocol

> **Scope:** the wire format that lets any client (another CodeTerm
> desktop, the relay binary, a mobile app) connect to a peer over the
> mesh and subscribe to a live pane — buffer snapshot, byte stream,
> structured events, optional input. This is the load-bearing piece;
> every other sub-plan consumes it.

Parent: [00-vision.md](00-vision.md).
Blocks: [20](20-provider-plugins-ts.md), [30](30-mobile-apps.md),
[50](50-relay-node.md).

## 1. Goals

- One protocol, three consumers (desktop-viewing-other-peer, relay SPA,
  mobile app). No surface invented per client.
- A reconnect after hours of being offline is the *normal* case, not an
  edge case. Cursors and snapshots are first-class.
- Cheap on the wire: live PTY bytes are the hot path; everything else is
  metadata or structured events at human typing rate.
- Capability-gated: `pane.read` and `pane.send_text` are distinct;
  clients can hold one without the other.
- Multi-pane subscriptions over **one** connection per peer (a phone
  watching 3 panes uses one socket, not three).

## 2. Non-goals

- No replacement for our REST API (`/api/...`) used by tooling and CLI.
  Transport is for *interactive subscription*; one-shot lookups stay
  REST.
- No browser-direct access from the public internet. Clients reach the
  peer over the tailnet (or via a relay node that itself sits on the
  tailnet — same trust boundary).
- No application-level retry of dropped bytes. PTY is best-effort; if a
  client gaps, it asks for a fresh snapshot.

## 3. Connection model

- **WebSocket over HTTPS**, terminated on the peer's existing HTTP
  surface (`/api/transport` or similar). One socket per (client, peer)
  pair. Multiple pane subscriptions multiplex inside.
- **HTTP/2 not strictly required.** WebSocket is enough; we want
  ordered frames per pane and back-pressure, and WebSocket gives both.
- **Auth at connect time only.** The mesh peer pairing already
  establishes a shared identity (Ed25519). The transport upgrade is
  signed with the same key. After upgrade, the socket carries the
  client's `peer_id` and the set of capabilities granted to it.
- **One socket = one subscriber bag.** Subscriptions live on the socket;
  if the socket dies, all subs die with it. Reconnect re-subscribes
  explicitly (with last-seen cursors).

## 4. Resource model

The address space the client navigates:

```
mesh
└── peer_id                  ← stable, Ed25519-derived
    └── workspace_id         ← stable per peer
        └── pane_id          ← stable per workspace
            ├── pty_stream       (raw bytes)
            ├── events_stream    (structured: status, agent events)
            └── metadata         (title, agent_type, cwd, started_at, ...)
```

- Listing peers / workspaces / panes stays on **existing REST**
  (`/api/mesh/peers`, `/api/workspaces`, `/api/panes`). Transport
  itself doesn't enumerate — it subscribes by id.
- A pane's id is **globally unique within the mesh** (peer_id + pane_id
  composes if needed; current pane_id is already UUID-ish, fine).

## 5. Message frames

All frames are JSON for v1. Binary (msgpack or framed PTY chunks) is a
v2 optimisation; we don't need it for the data rates involved (kilobytes
per second per pane, dozens of panes max).

### Client → Server

```jsonc
// Subscribe to a pane. last_seen=null means "first time, give snapshot".
{ "op": "subscribe",
  "sub_id": 17,                    // client-assigned, used to demux server frames
  "pane_id": "...",
  "want": ["pty", "events"],       // subset of stream kinds
  "last_seen": { "pty": 124530, "events": 89 }  // or null
}

// Send input to a pane (requires pane.send_text capability).
{ "op": "send_text",
  "pane_id": "...",
  "text": "ls -la\n",
  "request_id": "abc"              // client correlates the ack
}

// Drop a subscription.
{ "op": "unsubscribe", "sub_id": 17 }

// Keepalive when idle. Server sends one too every 30s.
{ "op": "ping", "ts": 1717689600000 }
```

### Server → Client

```jsonc
// Snapshot response to subscribe — current state + recent events.
{ "type": "snapshot",
  "sub_id": 17,
  "pane_id": "...",
  "buffer": {
    "rows": 50, "cols": 200,
    "screen": "<vt100-replayed text>",       // already-replayed buffer, no ANSI to apply
    "scrollback_tail": "<last N rows above viewport>"
  },
  "status": { "kind": "agent_working", "agent_type": "claude", ... },
  "events_tail": [/* last K structured events */],
  "cursors": { "pty": 124530, "events": 89 }
}

// Live PTY bytes — small, frequent.
{ "type": "pty",
  "sub_id": 17,
  "seq": 124531,
  "bytes": "<base64 or raw utf-8 chunk>"
}

// Structured event from the pane (status change, agent event, exit, prompt).
{ "type": "event",
  "sub_id": 17,
  "seq": 90,
  "event": { "kind": "agent_done", "payload": { ... } }
}

// Ack to send_text (or error).
{ "type": "ack",
  "request_id": "abc",
  "ok": true,
  "submitted_at_seq": 124533       // pty cursor right after the submit echo
}

// Subscription died server-side (pane closed, peer revoked capability).
{ "type": "sub_closed",
  "sub_id": 17,
  "reason": "pane_exit" | "permission_revoked" | "peer_shutdown"
}

// Heartbeat.
{ "type": "pong", "ts": 1717689630000 }
```

### Cursor / replay semantics

- Two cursors per pane: **pty** (monotonic seq of byte chunks) and
  **events** (monotonic seq of structured events). They are independent.
- `subscribe` with `last_seen = { pty: N, events: M }` means *"resume
  from after these"*. Server responds:
  - if its retained buffer / event log still goes back that far →
    delta frames from N+1 / M+1 onward, no snapshot frame.
  - otherwise → `snapshot` frame with current state + fresh cursors;
    the client throws away its stale state.
- Server retention is **best-effort and short**: ~5 minutes of PTY
  chunks, ~1000 recent events. Beyond that = snapshot. This keeps the
  daemon's memory footprint bounded.

## 6. Structured event vocabulary

Events the server emits on the `events` stream. Each has a `kind`,
`pane_id`, and a `payload` whose shape depends on `kind`. This is the
*minimum* set v1 must support; provider plugins add more via the
extension mechanism in [20-provider-plugins-ts.md](20-provider-plugins-ts.md).

| Kind | When emitted | Payload |
|---|---|---|
| `pane_created` | Pane opens (rarely seen by subscribers since they subscribe after) | `{ title, cwd, agent_type? }` |
| `pane_title_changed` | OSC 0/2, manual rename, agent session name pinned | `{ title, source }` |
| `pane_status_changed` | Status transitions (idle → agent_working → waiting_input → done → ...) | `{ from, to, agent_type? }` |
| `pane_exit` | Process terminates | `{ exit_code, duration_ms }` |
| `agent_prompt` | Provider asks for permission / option-pick | `{ question, options: [{label, value}] }` |
| `agent_event` | Provider-specific event (token usage, tool call, etc.). Opaque to core. | `{ provider, raw: <plugin-defined> }` |

`agent_event` is the **extension hole** — a provider plugin parses
JSONL session data and emits these so the renderer plugin can pick them
up over transport without core caring about their shape.

## 7. Capabilities

A client connection carries a set of capability strings. Initially:

- `pane.read` — receive snapshot + pty + events for any pane
- `pane.send_text` — call `send_text`
- `pane.subscribe_events` — receive `event` frames (separable from
  pty for low-bandwidth dashboards)
- `pane.list` — call REST enumeration endpoints

The peer's pairing record stores the set granted to each remote peer.
Default for newly-paired peers: `pane.read` + `pane.list`. Write
capability is opt-in (UI prompt at pair time, editable later).

Capability check on every operation, not just at connect. Revocation is
immediate: the operation returns `sub_closed` with reason
`permission_revoked`.

## 8. Send-text safety

Writing to someone else's pane is the dangerous half. v1 rules:

- `send_text` is **rate-limited** per (source peer, target pane):
  default 10 calls / minute. Configurable per pairing.
- Server logs every `send_text` to the activity log on the target peer
  with source peer_id, text preview, and ack outcome. Visible in the
  Mesh tab.
- No client-side `Enter` ambiguity: the protocol treats `text` as
  exactly what should be written to PTY input. If the client wants
  Enter, it includes `\n`. No magic.
- An optional `confirm: true` flag on `send_text` makes the target
  peer raise a UI prompt before executing. Mobile clients should pass
  this for destructive-looking commands (heuristic in the app, not the
  protocol). Server policy can also force confirm regardless.

## 9. Multiplexing and back-pressure

- All subscriptions share one socket. The server is allowed to interleave
  frames from different `sub_id`s freely.
- Per-subscription **byte budget**: if a client doesn't drain (e.g.
  mobile in a tunnel), the server buffers up to ~256 KB per pty
  subscription, then drops the oldest chunks and sets a `gapped` flag
  on the next frame. Client treats `gapped` as "ask for a fresh
  snapshot now".
- Events stream is never dropped — it's tiny. If the events buffer
  fills (shouldn't happen at typical rates), the server `sub_closed`s
  with reason `event_overflow`; client re-subscribes.

## 10. Versioning

- The first frame the server sends after upgrade is
  `{ "type": "hello", "protocol_version": 1, "server_build": "..." }`.
- Client checks; if it doesn't support the version, it closes.
- Adding new event kinds / new ops / new optional fields is **not** a
  version bump. Removing or changing semantics is.

## 11. Acceptance criteria for M1 (transport MVP)

The minimum that lets us delete the "I'll figure out the protocol later"
rectangle:

- [ ] Desktop CodeTerm exposes `/api/transport` WebSocket on the dev
      port (7686). Auth: bearer token = current dev token.
- [ ] Subscribe to a pane, receive snapshot, see live PTY bytes,
      receive `pane_status_changed` and `pane_exit` events.
- [ ] Send-text round-trip with ack; rate-limit enforced.
- [ ] Reconnect with cursors → delta replay if within window, fresh
      snapshot otherwise.
- [ ] CLI tool `codeterm pane attach <peer> <pane>` that opens the
      socket and renders the pane in the local terminal (raw passthrough
      for v1). This is both a dogfooding tool and the integration test.

## 12. Open questions (decide while implementing)

1. **Buffer encoding in snapshot.** Send pre-replayed text + cursor
   position? Or a tiny VT100 state object? Pre-replayed is simpler;
   state object lets the client resize freely. Lean: pre-replayed
   for v1, state object as an additive field later.
2. **`scrollback_tail` size policy.** How much scrollback do we include
   in the snapshot — last viewport, last 1000 rows, whole history?
   Compromise: send last viewport synchronously, expose a
   `request_scrollback` op for explicit pulls.
3. **Per-pane secrets.** Some panes contain credentials. Do we mark
   them `transport_disabled`? Probably yes — config flag on pane,
   server refuses subscribe if set. Decide at M1 or M2.
4. **JSON vs MessagePack.** Stay JSON until we measure pain. Mobile on
   spotty cell may want binary; revisit at M5 if there's data.

## 13. First step

A single Rust module `src-tauri/src/transport/` with:
- WebSocket handler wired into the existing HTTP server
- `Subscription` struct that pulls from the existing `OutputBuffer` and
  `PaneStatusTracker`
- Bearer-token auth using existing dev/prod token machinery
- The 5 client → server ops and 6 server → client frame types above

No new crates, no new dependencies beyond `axum`/`tokio-tungstenite`
which we already have. The whole thing should fit in <500 LOC for v1.

<!-- revision: 1 -->
