# 30 — Mobile apps (iOS / Android)

> **Scope:** native iOS and Android applications that join the user's
> mesh as full peers, speak the pane transport protocol over Tailscale,
> and render panes using the same provider plugins as desktop. The phone
> is just another peer — not a "messenger client", not a Telegram
> replacement.

Parent: [00-vision.md](00-vision.md).
Blocked by: [10-pane-transport-protocol.md](10-pane-transport-protocol.md).
Consumes: [20-provider-plugins-ts.md](20-provider-plugins-ts.md),
[40-push-gateway.md](40-push-gateway.md).

## 1. Why native (not just PWA)

Decided up front so we don't re-litigate:

- **Push notifications** require APNs (iOS) / FCM (Android) and need a
  signed app on the device. PWAs can do web push on Android, not on
  iOS reliably.
- **Tailscale integration** on iOS uses Network Extensions —
  installable only via App Store. We don't need to bundle Tailscale
  (user installs the official Tailscale app and the phone joins their
  tailnet); we just need the app to be on a device that has
  Tailscale, which is friction-free in the store flow.
- **Biometric unlock** for the app (Face ID / fingerprint) is a native
  affordance that matters because the app can send commands to live
  agents.
- **Deep links** from push notifications open the app to the right
  pane — needs URL scheme registration only native apps get.
- **Store distribution** turns "set up the codeterm phone client" into
  "install one app". The user has Apple + Google dev certs, no
  blocker.

Bonus: with native shell we still **render the UI in a WebView** and
reuse the same React code + provider plugins as the desktop. So
"native" here means a thin Swift/Kotlin app whose main view is a
WebView, plus native bridges for: Tailscale presence check, biometrics,
notifications, secure storage. The interesting code stays portable.

## 2. Architecture

```
iOS / Android app
├── native shell (Swift / Kotlin)
│   ├── biometric unlock gate
│   ├── secure storage (Keychain / Keystore) — mesh keypair, push token
│   ├── push notification handler → deep link → WebView route
│   └── WebView host
└── WebView contents (React, shared with desktop)
    ├── transport client (WebSocket to peer over tailnet)
    ├── provider plugins (same bundles as desktop)
    ├── pane viewer (xterm.js + plugin renderer)
    └── peer/pane/workspace browser
```

The phone is provisioned as a regular mesh peer. Its `device_id`
(Ed25519) lives in Keychain/Keystore. It pairs with desktop peers via
QR code (same pairing flow CodeTerm already has, plus a screen on
desktop that says "scan from phone").

## 3. App lifecycle

- **Foreground:** WebSocket connection to whichever peer the user is
  viewing. Cursor-resume from last seen if reconnecting.
- **Background:** connection drops. Nothing held open. Battery untouched.
- **Push arrives:** OS wakes the app to handle the push (silent or
  user-visible per event kind). Payload contains
  `{ peer_id, pane_id, kind }`. Tap → app opens to that pane → fresh
  subscribe with `last_seen = null` → snapshot + live.
- **App reopened after hours:** routes to last-viewed pane (or peer
  list), reconnects, resyncs.

There is no "background sync." The contract with the OS is honest: we
only do work when the user wants us to.

## 4. UI surfaces (v1)

Bare-minimum, scoped tight:

| Screen | What it shows |
|---|---|
| Splash / unlock | Biometric gate; on fail → passcode |
| Peers list | All mesh peers (online indicator from REST `/api/mesh/peers`) |
| Workspaces (per peer) | List of workspaces on selected peer |
| Panes (per workspace) | List of panes with title + status badge + agent_type icon |
| Pane viewer | Either fancy chat (plugin renderer) or raw xterm; toggle |
| Send | Input field at bottom of pane viewer; confirm modal for destructive heuristics |
| Settings | Pairing QR scan, push event subscriptions, manage paired peers |

No agent spawning from the phone in v1. No mem search. No workspace
management. Just **read panes, send text, get pushed when something
matters.**

## 5. Pairing flow

1. User on phone: Settings → Pair new peer → opens camera.
2. User on desktop: Mesh tab → "Pair phone" button → renders QR with
   `{ peer_id, paired_to_propose_pubkey, pairing_secret, listen_addr }`.
3. Phone scans, posts a pairing request to the desktop peer's tailnet
   address, completes Ed25519 handshake. Desktop UI shows
   "approve / reject" — user taps approve.
4. Capability defaults granted to the phone: `pane.read`,
   `pane.list`, `pane.subscribe_events`. **`pane.send_text` is
   opt-in** during the approve dialog, defaulting OFF. A separate
   tap-to-enable in the Mesh tab later. Same flow scales to
   multiple phones / tablets per user.

Pairing reuses the existing mesh pairing machinery (see
`docs/MESH.md`), with the phone as a `PeerKind::Mobile` variant on the
peer record so other peers can render it appropriately ("phone" icon,
not "machine").

## 6. Transport client

The same TS transport client used by desktop's "view other peer's
pane" feature. Imported from `@codeterm/transport-client` package
shared between desktop, mobile WebView, and relay SPA.

Cursor persistence: `last_seen` cursors per pane are stored in
WebView's IndexedDB. Backgrounding the app preserves them; a tap into
a pane sends the last cursor to ask for a delta.

## 7. Provider plugin loading

- Plugin bundles are fetched from the **peer** the phone is currently
  viewing. The peer hosts `/api/plugins/<id>/dist/plugin.js`. Phone
  caches by `(id, version)`.
- Rationale: each peer knows which plugin versions are compatible with
  the panes it's running. The phone shouldn't have to track desktop
  plugin updates separately.
- Same fallback rule as desktop: if no plugin → xterm passthrough.
  Always something to show.

## 8. Push subscription

(Details live in [40-push-gateway.md](40-push-gateway.md). The mobile
side:)

- On first launch, app requests push permission, gets APNs/FCM device
  token, posts it to the user's push gateway peer (during pairing the
  gateway peer is discoverable via mesh).
- App offers checkboxes per event kind per peer: "Push me when
  BLACKSTAR's agent waits for input" etc. Stored on the push gateway
  side, not on the phone.
- Pushes carry `{ peer_id, pane_id, kind, ts }` only. No content. App
  fetches content on tap.

## 9. Distribution

- **iOS:** App Store. Bundle id `com.codeterm.mobile`. User signs in
  to App Store, installs, pairs. No internal distribution dance.
- **Android:** Google Play. Same shape.
- **TestFlight / Play internal track** for dogfood builds during
  development.

No web fallback yet (the relay's SPA in [50](50-relay-node.md) covers
that case anyway).

## 10. Tech choices

Two reasonable paths; lean and reason:

### Option A — Tauri Mobile (lean: NO for v1)

- Pros: same Tauri toolchain, share commands with desktop, one codebase.
- Cons: still alpha on iOS, biometric/push integration patchy, store
  submission story not battle-tested. We'd be holding the maturity
  bag.

### Option B — Swift (iOS) + Kotlin (Android) shells, shared WebView ✅

- Pros: native APIs are first-class, store submission is well-trodden,
  the WebView contents are 95% of the code and stay shared.
- Cons: two native shells to maintain (small surface; ~500 LOC each
  for v1).

**Lean: Option B.** The native shells are tiny precisely because we
push everything into the WebView. The shared React code does the work.

## 11. Acceptance criteria for M5 (read-only mobile)

- [ ] iOS app installs from TestFlight, opens, biometric unlock works.
- [ ] Pairs with desktop CodeTerm via QR; appears in desktop Mesh tab.
- [ ] Lists peers / workspaces / panes from the paired desktop.
- [ ] Opens a pane, sees fancy chat (Claude plugin renders).
- [ ] Backgrounds, returns 10 min later, taps pane → resumes.
- [ ] Android parity for the same scope.
- [ ] Send-text **explicitly disabled** in v1 build for safety.

## 12. Acceptance criteria for M6 (send + push)

- [ ] `pane.send_text` capability toggleable per pair, defaults OFF.
- [ ] When enabled, send input from phone works with ack visible.
- [ ] Push gateway integration: phone receives wake-up on
      `agent_waiting_input`, taps → opens pane → sees the question.
- [ ] Confirm modal for destructive-looking input (heuristic in app,
      tunable list).

## 13. Open questions

1. **Tailscale dependency UX.** Do we put "Install Tailscale" as a
   first-run step in the codeterm app, or assume the user has it? Lean:
   detect at launch, render a single-screen "you need Tailscale, here's
   why, here's the App Store link" with a deep link.
2. **Offline mode.** Show last-seen state of panes when the phone
   isn't on the tailnet? Probably yes for v1.1 — IndexedDB already has
   the cursor + last buffer. UI says "offline, last updated 12 min
   ago".
3. **Multiple phones per user.** Already supported by the mesh model
   (each phone = own peer_id). Just confirm the push gateway handles
   it (it does — see [40](40-push-gateway.md)).
4. **iPad.** Same iOS binary, just render with wider layouts. Not a
   separate target.

## 14. First step

Before any Swift/Kotlin: stand up a **PWA harness** that loads the
shared WebView code in a regular mobile browser, points at a desktop
peer's transport over the tailnet (the desktop already has a self-
signed certificate; we accept it in the PWA for dev). This validates
the WebView code on real phones without any store submission yet.
Once the PWA works end-to-end, wrap it in native shells.

<!-- revision: 1 -->
