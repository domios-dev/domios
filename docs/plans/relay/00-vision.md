# CodeTerm Relay — vision and master plan

> **One-line:** stop using messengers as portals to our agents. Build a
> first-class portal that natively understands panes, peers, providers, and
> our mesh — and lets us reach any pane on any peer from any device.

## 0. Why now: messengers are dumb design for this

We have a Telegram bridge today. It works, and it's a dead end. The
diagnosis:

- **A chat UI is wrong for terminal output.** A pane is a live buffer with
  reflow, ANSI, focus, partial updates. A chat is append-only with a small
  viewport. Anything intermediate (tool calls, thinking, file diffs,
  multi-step agent reasoning) gets either dumped as a wall of text or
  silently lost. We currently lose almost everything except the final
  short report — the entire point of running an agent is invisible.
- **Third-party transport in the loop.** Messages traverse external
  servers we don't control, can't audit, can't extend. Push delivery is
  the only thing that actually justifies the dependency.
- **Identity is wrong.** A `@username` is a person; we want to address a
  *pane on a peer*, with capability-scoped auth. Messenger identity is
  one-way (you're you, the bot is the bot) — no concept of "this device
  can read these panes but not those."
- **We already have a better transport.** Mesh (Tailscale-routed, flat
  peer model, mutual pairing, signed messages, push tokens) covers
  everything a messenger gave us except the final mile to a backgrounded
  phone.

Telegram is fine as a *push channel* (wake the phone). It is not fine as
the interface to a multiplexer running 12 agents across 4 machines.

## 1. The apex: codeterm-relay

A new participant in the mesh whose only job is **to be a portal**: it
joins the tailnet, pairs with full peers via Ed25519, and exposes:

- a **wire protocol for pane transport** — subscribe to any pane on any
  peer, get a snapshot + live stream, send input back (capability-gated)
- a **mobile/web client** that renders that transport — sees the tree of
  `peers > workspaces > panes`, taps into one, gets a live view
- an **inbound webhook surface** — external systems (Stripe, GitHub,
  email, anything) post into a relay endpoint and the message lands in a
  named pane on a named peer
- a **push gateway** — when something pane-worthy happens, the relay (or
  any peer with credentials) sends an APNs/FCM wake-up to native phone
  apps

Reference issue: **#62** ([codeterm-relay — lightweight mesh node for
inbound webhooks + remote UI](https://github.com/rollacode/codeterm/issues/62))
— this captures the relay binary itself (P0–P6 in that issue). This
master plan **broadens the scope** beyond just the relay binary: the
transport, the provider plugin system, the mobile apps, and the push
gateway are all parts of the same picture.

## 2. The DAG — what blocks what

```
                          ┌──────────────────────────┐
                          │ 50-relay-node            │  ← issue #62, the
                          │ (Rust binary + SPA)      │     "portal" binary
                          └──────────┬───────────────┘
                                     │ depends on
                ┌────────────────────┼──────────────────────┐
                ▼                    ▼                      ▼
   ┌────────────────────────┐  ┌──────────────────────┐  ┌────────────────────────┐
   │ 10-pane-transport      │  │ 20-provider-plugins  │  │ 40-push-gateway        │
   │ -protocol              │  │ (TS, hot-loaded)     │  │ (APNs/FCM as peer)     │
   │                        │  │                      │  │                        │
   │ wire format for        │  │ fancy chat rendering │  │ domain events →        │
   │ subscribe / snapshot / │  │ per provider, ships  │  │ native phone wake-up   │
   │ stream / send / events │  │ separately from core │  │                        │
   └─────┬──────────────────┘  └─────┬────────────────┘  └──────┬─────────────────┘
         │                            │                          │
         │                            │                          │
         ▼                            ▼                          ▼
   used by ALL clients:         used by desktop UI         needed for mobile to
   desktop, mobile, relay       AND relay SPA              be useful when closed
         │
         ▼
   ┌──────────────────────────┐
   │ 30-mobile-apps           │
   │ (iOS/Android native)     │
   │                          │
   │ thin clients consuming   │
   │ transport over tailnet,  │
   │ no relay required for    │
   │ basic use                │
   └──────────────────────────┘
```

**Reading the DAG:**

- The **pane transport protocol (10)** is the load-bearing piece. Every
  other consumer (relay, mobile, desktop-viewing-other-peer) speaks it.
  This is the work that, if done right, makes the rest cheap.
- **Provider plugins (20)** are orthogonal but synergistic. Pluggable
  per-provider rendering benefits the desktop today, and means the relay
  SPA and the mobile app get fancy chat rendering for free instead of
  reimplementing parsers per surface.
- **Mobile apps (30)** don't require the relay binary. A phone with
  Tailscale can be its own mesh peer and speak transport directly to
  other peers. The relay is a convenience (always-on portal) but not a
  prerequisite for mobile.
- **Push gateway (40)** is what lets the mobile app stay useful when
  closed. It's a thin service any peer can run.
- **The relay binary (50)** is the convenience wrapper that unifies all
  of the above into a single shippable "portal node" — webhook intake,
  hosted SPA, runs on a NAS / Pi / VM, always-on.

## 3. Order of work — what to build first

The goal at every step is to land **something usable on its own**, not a
big bang at the end.

| # | Milestone | Concretely usable for |
|---|-----------|------------------------|
| **M1** | Pane transport MVP — subscribe + raw PTY stream, no auth beyond mesh pairing | Desktop CodeTerm A can view a pane on Desktop CodeTerm B over tailnet. Validates the whole concept. |
| **M2** | Structured events on the transport (pane status, agent_type, exit codes, prompts) | Better signals than just bytes; foundation for push events |
| **M3** | First TS provider plugin (Claude renderer) extracted from frontend | Desktop chat is now plugin-driven; same plugin reusable in relay SPA |
| **M4** | Push gateway peer — APNs sender with one device-token registered manually | Phone gets a wake-up when M2 says "agent done" |
| **M5** | Mobile app skeleton — read-only pane viewer, tailnet-connected | "I can see my agents from my phone" |
| **M6** | Mobile app: send input + confirm flow | "I can talk back" |
| **M7** | Relay binary P1+P2 from issue #62 (pair, /api/send, webhook routing) | External webhooks land in panes — Stripe → agent |
| **M8** | Relay SPA (hosted version of mobile UI) | Anyone with the relay URL has a portal |

M1–M3 are the **load-bearing work**. M4–M8 build on top and can ship
incrementally without blocking each other.

## 4. Non-goals (explicit)

To prevent scope creep when reading the sub-plans:

- **No replacement for Tailscale.** We do not write our own VPN/relay
  network. Tailnet is the trust boundary.
- **No multi-tenant SaaS.** Each user runs their own relay node. The
  binary is designed so multi-tenant is *possible* later, not required
  now.
- **No web-based "open relay in any browser".** v1 requires being on the
  user's tailnet (Tailscale Funnel is a deliberate opt-in for public
  webhook endpoints only, not for the SPA).
- **No replacement for individual messengers.** Telegram bridge stays for
  conversational use; relay is for the *terminal portal* use case.

## 5. Cross-cutting decisions made up front

These are decided once here, applied in all sub-plans, not re-litigated:

- **Provider plugins are TypeScript**, not Rust/wasm/Python. Rationale:
  renderer must live in WebView (React), having parser in the same
  language avoids two-package sync. Hot-reload in dev via Vite HMR. Ship
  as pre-built JS bundle in prod. Detection patterns (regex/string match
  on PTY bytes) and JSONL parsers move out of Rust into the plugin.
- **Plugin runtime tiers**: WebView for UI-side plugin code; embedded
  QuickJS in the daemon for server-side hooks that must run when no
  WebView is open (later milestone, not M1).
- **Transport is push-based** (WebSocket / SSE), not poll. Resync on
  reconnect is a first-class flow, not an edge case — mobile clients are
  assumed to drop the connection when backgrounded.
- **No background sync on mobile.** When app foregrounded → connected.
  When closed → silent except for explicit push events. This eliminates
  battery concerns and the need for any background-mode protocol.
- **Push notifications are domain events**, not transport heartbeats.
  Examples: `agent_done`, `agent_waiting_input`, `agent_error`,
  `pane_exit`. The peer that owns the pane decides which events warrant
  a push and emits them to whichever peer is running the push gateway.

## 6. Sub-plans (separate documents)

Each is a self-contained scope with acceptance criteria, open questions,
and concrete first steps:

- [10-pane-transport-protocol.md](10-pane-transport-protocol.md) — wire
  format for cross-peer pane access (subscribe, snapshot, stream, send,
  events)
- [20-provider-plugins-ts.md](20-provider-plugins-ts.md) — TypeScript
  plugin system for provider rendering, amendment to the existing
  [plugin-architecture.md](../plugin-architecture.md) (TS-first path)
- [30-mobile-apps.md](30-mobile-apps.md) — iOS/Android native apps as
  tailnet-connected mesh peers
- [40-push-gateway.md](40-push-gateway.md) — APNs/FCM sender peer
  service, domain-event push model
- [50-relay-node.md](50-relay-node.md) — the relay binary itself,
  incremental over issue #62

## 7. What this plan replaces / supersedes

- **Old "Phase 5: Web UI" thinking** — the relay is a cleaner shape than a
  general "web UI bolted onto desktop CodeTerm".
- **Tightening, not replacing, [plugin-architecture.md](../plugin-architecture.md)** —
  we keep the Rust plugin skeleton (manifest, registry, lifecycle) but
  the practical first external plugins are TS, not Process/Wasm.
- **Telegram-as-primary-bridge ambitions** — Telegram drops to push-only
  or is retired entirely once the mobile push gateway ships.

<!-- revision: 1 -->
