# 50 — Relay node (codeterm-relay)

> **Scope:** the apex binary. A small always-on Rust process that joins
> the mesh as a peer, exposes inbound webhooks for external systems,
> and hosts a browser-accessible SPA that lets any device on the
> tailnet (or via opt-in Funnel) browse and interact with panes across
> all paired peers. Builds on top of every other sub-plan instead of
> reinventing the wheel.

Parent: [00-vision.md](00-vision.md).
References: GitHub issue **#62**
([codeterm-relay — lightweight mesh node](https://github.com/rollacode/codeterm/issues/62)).
Blocked by: [10](10-pane-transport-protocol.md),
[20](20-provider-plugins-ts.md), [40](40-push-gateway.md).
Optional: can co-host the push gateway from [40](40-push-gateway.md).

## 1. What's different from issue #62

Issue #62 describes the relay as a self-contained design with its own
REST surface (`/api/panes/:id/output` SSE, `/api/send`, etc.) and an
embedded SPA. That was right when written — the underlying primitives
didn't exist yet.

Now that we're committing to the **pane transport protocol** ([10](10-pane-transport-protocol.md))
and **TS provider plugins** ([20](20-provider-plugins-ts.md)), the relay
is recast as:

- A peer that **uses** the transport protocol internally rather than
  inventing per-pane SSE endpoints.
- An SPA that **embeds** the same React + provider plugins used by
  desktop and mobile — the relay doesn't ship a parallel renderer.
- A webhook layer that **emits domain events** the push gateway can
  pick up (or routes directly to a pane via `send_text`).
- An optional **push gateway host** (the relay is a convenient place to
  put the .p8 cert because it's always-on by definition).

What stays from #62 verbatim: TOML route table, capability-restricted
peer model, Tailscale Funnel as an opt-in public exposure, "NOT
Next.js" — single Rust binary + embedded SPA via `include_dir!()`.

## 2. The shipping shape

```
codeterm-relay (single binary, ~10–15 MB)
├── mesh peer (uses codeterm-mesh-client crate extracted from src-tauri/)
│   ├── pairs with full peers via Ed25519
│   ├── declares PeerKind::Relay
│   └── capability set: pane.read, pane.send_text, pane.list, event.subscribe
├── transport client
│   └── opens WebSockets to paired peers on demand (per SPA user session)
├── webhook intake
│   ├── HTTP server on configurable port (default 7687)
│   ├── routes per route_table.toml → pane on peer (via send_text)
│   ├── auth: HMAC by default, per-route OAuth/bearer as needed
│   └── emits relay-internal events (webhook_received, route_matched)
├── domain event projector
│   └── forwards mesh events + webhook events to push gateway (if configured)
├── embedded SPA
│   ├── React, same code as desktop's pane viewer
│   ├── loads provider plugin bundles from paired peers
│   └── served from /  with auth (bearer cookie from /api/login)
└── optional push gateway feature flag
    └── if .p8/FCM creds present, this binary IS the push gateway too
```

## 3. Crates extraction (P0 from #62)

Refactor work needed to make the relay possible at all:

- `crates/codeterm-mesh-client` — pulled out of `src-tauri/src/mesh/`.
  Headless mesh participant. Used by relay, desktop, mobile native
  shell.
- `crates/codeterm-pane-api` — pulled out of REST handlers + the new
  transport client from [10](10-pane-transport-protocol.md).
- `crates/codeterm-relay` — new, depends on the two above + `axum` +
  `include_dir`.
- `src-tauri/` keeps depending on the extracted crates rather than
  containing the code inline.

This is the **largest** dependency for relay shipping. It's tracked in
issue #62 as P0 and stays that way.

## 4. Webhook routing (kept from #62)

`route_table.toml` shape (no changes from #62):

```toml
[routes.stripe-failed]
match.path = "/webhook/stripe"
match.json_path = "$.type"
match.json_value = "payment_intent.payment_failed"
target.peer = "BLACKSTAR"
target.pane = "billing"
template = "❌ Stripe failure: {{json.data.object.amount}} for {{json.data.object.metadata.user_email}}"

[routes.github-pr]
match.path = "/webhook/github"
match.header.X-GitHub-Event = "pull_request"
target.peer = "BLACKSTAR"
target.pane = "general"
template = "📝 New PR: {{json.pull_request.html_url}} by @{{json.pull_request.user.login}}"
```

How it executes:

1. HTTP POST hits the relay.
2. Route table matched on (path, headers, JSON path).
3. Handlebars template rendered.
4. Relay calls `send_text` on transport against target peer/pane.
5. Optionally emits a `mention` domain event for the push gateway.

Auth per route: HMAC verification configurable per source (Stripe's
`Stripe-Signature`, GitHub's `X-Hub-Signature-256`, etc.).

## 5. SPA — what it actually is

Same code as the desktop pane viewer, packaged as a static bundle and
served by the relay over its own HTTP port. Same provider plugins.
Same transport client.

The only relay-specific UI:

- **Login screen** — bearer cookie auth (single tenant for v1; the
  relay represents one user / one mesh).
- **Peer picker** — populated from paired peers list.
- **Webhook activity log** — last N webhooks received, matched route,
  delivery outcome.

Everything else (workspaces, panes, fancy chat) is the same React
code mobile and desktop use.

## 6. Auth model

Three levels:

1. **Mesh-level** (peer → peer): Ed25519 pairing, same as any peer.
   The relay grants the full peers capability to push events to it
   (for the push gateway role), and is granted by the full peers
   `pane.read` / `pane.send_text` so it can serve the SPA.
2. **HTTP-level** (user → relay): bearer token, set in
   `~/.codeterm-relay/config.toml`. The SPA prompts for it on first
   visit, stores it as HttpOnly cookie. Future: OAuth via the user's
   chosen identity provider for multi-user / shared relay scenarios.
3. **Webhook-level** (external → relay): per-route HMAC / shared
   secret. Defaults to "reject all unauthenticated".

## 7. Tailscale Funnel opt-in

```bash
codeterm-relay run --tailscale-funnel
```

Exposes the relay on a public HTTPS URL via Tailscale Funnel. The
intended use case is **webhook intake only** — Stripe etc. need a
public URL to POST to. The SPA stays gated behind the bearer cookie
even when Funnel is on; we don't expose the pane portal to the public
internet by accident.

A separate `--funnel-allow-spa` flag makes that explicit if you really
want it (e.g. for personal use from a coffee shop). Defaults to
false.

## 8. Distribution

- **Raw binary** for Linux / macOS / Windows. NSIS installer not
  needed; this is a service, not a UI app.
- **Docker image** at `ghcr.io/codeterm/relay:latest` — the common
  case is "run on my NAS / Pi / VM". Image is small (~25 MB with
  alpine base).
- **systemd unit** template shipped in `dist/` for the bare-metal
  case.
- **Synology / Unraid app packaging** as a follow-up; if it runs in
  Docker it runs everywhere these NASes care about.

## 9. Phased delivery (recast from #62)

| P | Scope | Status / dependency |
|---|---|---|
| P0 | Extract `mesh-client` + `pane-api` crates | Refactor; can start immediately |
| P1 | Minimal relay binary: pair with peers, manual `send_text` via REST | Needs P0 |
| P2 | Webhook routing + TOML config + handlebars templates | Builds on P1 |
| P3 | Embedded SPA (peer/pane viewer using shared React + provider plugins) | Needs [10](10-pane-transport-protocol.md) + [20](20-provider-plugins-ts.md) |
| P4 | Tailscale Funnel one-command exposure + auth modes | Builds on P3 |
| P5 | Push gateway feature flag (co-host the gateway from [40](40-push-gateway.md)) | Needs [40](40-push-gateway.md) |
| P6 | Durable webhook queue when peer offline | Late polish |

"Killer demo" remains P1+P2 (~2 weeks of work after P0). That's:
external webhook → mesh peer's pane → agent reasons over it.

## 10. Acceptance criteria for M7 (relay P1+P2)

- [ ] `codeterm-relay run` joins mesh, lists in `codeterm mesh peers`
      from a full peer as `PeerKind::Relay`.
- [ ] Manual pairing flow completes between relay and desktop.
- [ ] `POST /api/send {peer, pane, text}` lands text in the target
      pane (with bearer auth).
- [ ] `POST /webhook/stripe` with a matching payload triggers the
      `stripe-failed` route → text appears in `billing` pane on
      `BLACKSTAR`.
- [ ] HMAC verification rejects unsigned webhooks.
- [ ] Activity log accessible via `GET /api/webhooks/log`.

## 11. Acceptance criteria for M8 (relay SPA)

- [ ] `GET /` serves the SPA after bearer login.
- [ ] SPA lists paired peers, drills into workspaces and panes.
- [ ] Tapping a pane streams its live view (transport from [10](10-pane-transport-protocol.md)).
- [ ] Claude panes render via the Claude plugin (from [20](20-provider-plugins-ts.md)).
- [ ] Send-text from SPA works (rate-limited).
- [ ] Webhook activity log visible inside SPA.

## 12. Open questions (from #62, plus new)

1. **Webhook auth presets.** Ship route templates for Stripe / GitHub
   / Slack / Linear with their auth schemes pre-wired. Decide which
   3–5 to ship in P5.
2. **Multi-tenant later?** v1 is single-mesh, single-user. The
   architecture must not block multi-tenant (we use scoped pairing
   per route table?). Sketch but don't build.
3. **Same-binary mode `codeterm --relay-only`?** No — separate
   binary, shared workspace. Bloating the desktop binary with
   webhook intake doesn't pay off.
4. **PWA installability of the SPA.** Nice-to-have; the mobile
   native app in [30](30-mobile-apps.md) is the better answer for
   real phone use. But a `manifest.json` lets the SPA "install" on
   iOS as a homescreen icon — basically free.
5. **Durable queue (P6).** Needed when the target peer is offline for
   hours and webhooks keep arriving (Stripe will retry, but bursts
   exist). Probably sqlite-backed. Defer until we observe the need.

## 13. First step

Open a tracking PR titled `chore(workspace): extract codeterm-mesh-client
+ codeterm-pane-api crates` doing **only** the refactor — no relay
code yet. This is the bridge from "everything in `src-tauri/`" to
"reusable crates", and lands value immediately for desktop too (clean
module boundaries, faster compile of unrelated changes). Once that's
in, scaffold `crates/codeterm-relay` as an empty axum service that
joins the mesh and answers `GET /healthz`. The relay milestones
proper start after that.

<!-- revision: 1 -->
