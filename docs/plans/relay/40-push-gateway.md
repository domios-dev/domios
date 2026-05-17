# 40 — Push gateway

> **Scope:** a peer service that holds APNs (iOS) and FCM (Android)
> credentials and turns mesh-internal domain events ("agent finished",
> "agent waiting input") into native push notifications on the user's
> phones. Any peer with the credentials can run it; the relay
> binary is one obvious host but not the only one.

Parent: [00-vision.md](00-vision.md).
Used by: [30-mobile-apps.md](30-mobile-apps.md),
[50-relay-node.md](50-relay-node.md).

## 1. The shape of the problem

Mobile OSes do not let an app maintain a long-lived connection in the
background. To reach a closed app you need a push notification, which
on iOS goes through APNs (Apple) and on Android through FCM (Google).
Both require:

- A signed cert (.p8 for APNs / service-account JSON for FCM) held by a
  server.
- An HTTPS request from that server to the OS push service with the
  target device's token.

The user has Apple + Google dev accounts, so the credentials are
obtainable. The question is **where they live** and **what triggers a
push**.

This plan's answer:

- **Where:** any mesh peer that has the credentials installed. We don't
  require a "single canonical push server". Typically one peer (a NAS,
  a Pi, or the relay node) holds them, but the protocol allows multiple.
- **What triggers:** explicit domain events emitted by peers,
  delivered to the push gateway peer via mesh, fanned out to the
  device tokens that subscribed to those events.

## 2. Architecture

```
peer with running agent
        │
        │ emits domain event:
        │   { kind: "agent_waiting_input", peer_id, pane_id, payload }
        ▼
   ┌───────────────────────┐
   │ mesh                  │  ← signed, ordered, peer-to-peer
   └─────────┬─────────────┘
             │
             ▼
   ┌───────────────────────────────────┐
   │ push gateway peer                 │
   │  - device token registry          │  ← phones registered here at pairing
   │  - subscription rules per device  │
   │  - APNs / FCM clients             │
   └─────────┬─────────────────────────┘
             │   POST APNs / FCM
             ▼
   ┌───────────────────┐    ┌──────────────────────┐
   │  iPhone           │    │  Android             │
   │  push notif       │    │  push notif          │
   └──┬────────────────┘    └──┬───────────────────┘
      │  tap                   │ tap
      ▼                        ▼
   open app → pane             open app → pane
```

The gateway is itself a peer (`PeerKind::PushGateway` for visibility in
mesh UI). It speaks the same pairing + capability model as any other
peer — phones grant it `push.send`, agent peers grant it
`event.subscribe`.

## 3. Event vocabulary (v1)

These are the only events that warrant a push initially. The list is
**closed** — we add new ones with intent, not on a whim, because each
push to a user is a small interruption.

| Kind | Meaning | Default priority |
|---|---|---|
| `agent_waiting_input` | A provider raised a permission prompt or option-pick that needs the user | High (sound, banner) |
| `agent_done` | A long-running agent completed its task | Normal (banner, no sound) |
| `agent_error` | A provider returned an error and stopped | High |
| `pane_exit_nonzero` | A non-agent pane exited with a non-zero code | Normal |
| `mention` | External system (webhook) routed a `mention`-tagged payload to a pane | High |

Each event carries `{ kind, source_peer_id, pane_id, ts }` and a tiny
`payload` (max 100 bytes after JSON). Payload examples:

- `agent_waiting_input` → `{ question: "Allow read?" }` (truncated)
- `agent_done` → `{ summary: "12 tests passed" }` (optional)

**Push notification body is built from kind + payload**, e.g.
"BLACKSTAR · claude on `~/code/foo` is waiting for input: 'Allow
read?'". Limited to 200 chars total.

## 4. Subscription model

Each device picks **what they want pushed**, not the gateway. Stored
on the gateway peer:

```jsonc
{
  "device_id": "<phone's mesh peer_id>",
  "apns_token": "...",                 // or fcm_token
  "platform": "ios" | "android",
  "subscriptions": [
    { "peer_id": "BLACKSTAR", "kinds": ["agent_waiting_input", "agent_error"] },
    { "peer_id": "*",         "kinds": ["mention"] }
  ]
}
```

Phone updates this via REST on the gateway peer (`POST
/api/push/subscriptions`) at first launch and whenever the user
toggles in app settings.

`peer_id: "*"` is the wildcard for "any peer in my mesh". Useful for
mentions and global pages, fine-grained for per-peer is opt-in.

## 5. Event emission

Each peer's existing internal status machinery already emits events
into the `EventBroadcaster` (see `src-tauri/src/api/events.rs`). Wiring:

- A new component **push event projector** subscribes to the event bus.
- When a relevant `SystemEvent` fires (e.g.
  `PaneStatusChanged{from: working, to: waiting_input}`), the
  projector builds a domain event and pushes it onto the mesh's
  signed-message channel addressed to the configured push gateway
  peer(s).
- The peer can be configured with **zero, one, or multiple** push
  gateway peers. Zero = disabled (no phones). One = normal.
  Multiple = redundancy (each gets the event; dedup on the device side
  by `(source_peer_id, pane_id, kind, ts)` already happens automatically
  because OSes coalesce identical pushes).

The projector is rate-limited: no more than 5 pushes per pane per
minute, no more than 30 per peer per minute. Coarse anti-spam.

## 6. Credentials handling

- `.p8` (APNs) and service-account JSON (FCM) sit on the gateway peer
  in `~/.codeterm/push/`.
- Encrypted at rest using the existing secrets store
  (`docs/plans/secrets-store.md`). Loaded into memory at gateway
  start, never logged.
- A `codeterm push setup` CLI walks the user through getting the
  credentials and dropping them in.

## 7. Reliability stance

We accept that push is **best-effort**:

- If APNs is down or the device is unreachable, the push is lost. The
  phone will see the event the next time it opens the app and
  re-syncs (the pane's status is still correct on the source peer,
  visible immediately on connect).
- No retry queue on the gateway. APNs already retries once internally;
  beyond that it's not worth holding state.
- If the gateway peer is offline when an event fires, the event is
  buffered on the source peer for up to 5 minutes; if the gateway
  comes back, the event is delivered. Beyond 5 minutes — dropped.
  This matches mesh delivery semantics in general.

The deliberate posture: **push is a convenience, not a reliable
delivery channel.** The phone always re-syncs on open and shows
correct current state. Missing pushes degrade experience but never
hide reality.

## 8. Privacy

- Push payload content is **truncated and minimal** by default.
  Sensitive panes can be marked `no_push_content`, in which case the
  push body is generic ("Pane on BLACKSTAR needs attention") and the
  actual content is only visible after biometric unlock + opening the
  app.
- Per-peer pairing capability `event.subscribe` controls whether the
  push gateway can even receive events from that peer. Granular
  enough to exclude a sensitive home peer from a phone the user
  also gives to family.

## 9. Acceptance criteria for M4

- [ ] `codeterm push gateway` runs as a service on any peer that has
      credentials installed.
- [ ] Phone can register a device token via REST.
- [ ] Triggering `agent_waiting_input` on a paired peer produces a
      push on the phone within ~5 seconds end-to-end.
- [ ] Rate limit holds: 20 rapid events → at most 5 pushes per minute.
- [ ] Credentials never appear in logs; the secrets store wraps them.

## 10. Open questions

1. **Self-hosted FCM / APNs proxy vs Apple's HTTP/2 endpoint
   directly.** Lean: HTTP/2 directly — simpler, no extra infra.
2. **Web Push (browser) as a third channel?** Useful for the relay's
   SPA on desktops without the codeterm app open. Nice-to-have v1.1;
   not required.
3. **Critical alerts (APNs Critical Alert entitlement) for
   `agent_error`?** Requires Apple's special permission; defer until
   we have real demand.
4. **Per-event quiet hours.** Configurable on phone side ("don't push
   `agent_done` between 23:00–07:00"). Phone-side filter, no gateway
   change. Add in M5+.

## 11. First step

Build the gateway as a standalone binary in the existing workspace —
`crates/codeterm-push-gateway`. It binds to mesh, accepts a single
event kind (`agent_waiting_input`), pushes to one hardcoded test
device token via APNs. Once the end-to-end signal lights up on a real
iPhone, expand to the full event vocabulary and subscription model.
This is ~300 LOC and a single Apple .p8 file from start to first push.

<!-- revision: 1 -->
