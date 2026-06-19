# Messenger — unified inter-pane message gateway (storage-aware routing + metadata)

> Owner: Dante. Brief from Andrey 2026-06-19 (+ 2 screenshots: GA sends to the
> lmstudio "Joyce/Nemotron" chatBackend pane → arrives as a BARE user bubble, no
> sender identity / return address). Branch off `plugins-rethink`.

**Problem:** `codeterm send --pane X` (and reports, mesh relays, delayed/reminder
sends) all build a text prefix via `CtxPrefix` (service/msg_prefix.rs):
`-=-codeterm:pane label=<A> pane=<id> model=<m>-=-body`. That assumes the
recipient is **PTY-backed** (the prefix is injected into the agent's terminal as
its return address). For a **chatBackend** (non-PTY) recipient the transcript
lives HOST-side, there's no PTY, and the send path
(`pane_transport_chat.rs:66` — "chatBackend must not get context injected")
strips the WHOLE staged block — conflating two different things:
- the **sender identity / return-address prefix** (must be KEPT — it's who-sent +
  how-to-reply + a render hint), and
- the preprocessor **`<context>` block** (cwd/env, only meaningful to an agent PTY
  pane — correctly stripped for a shell chat).
Result (screenshots): the message arrives with NO sender metadata → the renderer
shows a bare user bubble, and the chatBackend model can't tell who wrote it or
reply to the right address.

**Core insight (Andrey):** route on WHERE THE CHAT IS STORED. A PTY pane's
transcript is owned by the provider → deliver as injected text (prefix). A
chatBackend pane's transcript is owned by US (host-side JSONL, CB-P4) → store the
message directly host-side WITH structured sender metadata, and our renderer +
the model both consume it. One gateway decides the format per addressee.

## Design — promote `CtxPrefix` into a `Messenger` gateway

Rename `service/msg_prefix.rs` (`CtxPrefix`) → `service/messenger.rs`
(`Messenger`). The text-prefix becomes ONE rendering of a richer
`MessageEnvelope { from_label, from_pane, from_model, from_mesh?, kind
(pane|report|reminder|mesh|telegram), set_at?/fire_at? }`. Single entry point:

`Messenger::deliver(to_pane, envelope, body) ->` resolve the recipient's STORAGE
and dispatch:
1. **PTY recipient** (provider-stored): render the envelope as the
   `-=-codeterm:...-=-` text frame (exactly today's CtxPrefix output) + inject into
   the PTY. Strip the preprocessor `<context>` only for non-agent panes (unchanged).
2. **chatBackend recipient** (host-stored): write a structured inbound `ChatMsg`
   into the host transcript carrying the envelope as STRUCTURED fields (sender
   label/pane/model + kind), NOT a raw text prefix. Keep the identity; drop only
   the cwd/env `<context>`. The model still receives the sender context (a compact
   "from <label>: <body>" or a structured system note) so it knows the return
   address; the renderer gets structured fields to draw a proper "from X" header.

All senders go through it: `send --pane`, `hierarchy::submit_report`,
`mesh_routes/relay`, `orchestrator`, and `delayed_send` (at FIRE time — so a queued
reminder is delivered storage-aware too, reusing the T7 envelope which already
carries who-set/set_at/fire_at).

## Tracks

- **M1 — `Messenger` gateway (backend)** — branch `msg-m1-gateway`. Rename
  msg_prefix→messenger; introduce `MessageEnvelope` + `Messenger::deliver`;
  recipient-storage resolution (PTY vs chatBackend via `pane_chat_backing`);
  PTY path = current behavior; chatBackend path = store structured inbound ChatMsg
  with sender metadata (keep identity, strip only `<context>`). Route `send --pane`
  + report through it. TDD: a send to a PTY pane → text prefix injected (unchanged);
  a send to a chatBackend pane → host transcript gains a structured inbound msg with
  from_label/pane/model (NOT a bare body, NOT a leaked raw prefix). Gate: cargo
  build/test + hook clippy (src+cli).
- **M2 — chatBackend inbound rendering (frontend)** — branch `msg-m2-render`.
  Render a host-stored inbound inter-pane message with a "from <label> · <model>"
  header (mirror the mesh/report cards), from the structured fields. So the
  screenshot-1 message shows "from CodeTerm GA", not a bare bubble. TDD: renders
  the from-header; a normal user msg unchanged. Gate: tsc + test:unit.
- **M3 — route delayed/reminder + mesh through Messenger** — branch `msg-m3-routes`.
  delayed_send + mesh relay deliver via `Messenger::deliver` at fire/relay time
  (storage-aware). TDD: a delayed send to a chatBackend pane lands as a structured
  inbound msg at fire time. Gate: cargo build/test + hook clippy.
- **M4 — chatBackend reply-to-return-address** — branch `msg-m4-replyaddr`. The
  chatBackend model/loop can reply to the sender using the stored return address
  (pane/mesh) — so "reply to Joyce's sender" works like the PTY pane routing table.
  Plugin-mediated. TDD: a stored inbound with a return address → a reply routes
  back to it. Gate: as touched.

## Notes
- Reuses CB-P4's host-side transcript (JSONL) as the storage truth for chatBackend.
- The `-=-codeterm:pane-=-` marker stays the WIRE format for PTY; for host-stored
  panes the envelope is structured (no fragile text-prefix-in-transcript).
- Naming: `Messenger` (gateway) chosen over "MetaBuilder"/"messanger" — it ROUTES +
  stamps, not just builds a string. CtxPrefix's string-building becomes its PTY
  renderer.
- Boundary: generic — the gateway branches on STORAGE (PTY vs host), never on a
  provider id.

## M1b — gateway robustness (live bugs, TDD, NO band-aids)

Three bugs found in live testing (Andrey 2026-06-19) — the gateway is close but
not solid yet. Investigated in `service/messenger.rs`:

**B1 — duplicate inbound.** `deliver_chatbackend` (messenger.rs:241) writes the
message TWICE: (a) `history.upsert(structured_msg)` + `emit_chat_msgs` (:265-268)
— the structured "from X" entry the renderer shows; AND (b) `chatbackend_send(staged)`
(:277) where `staged` = the `[CodeTerm inbound …]` header + body → the plugin
records it → the poll loop re-emits it as a SECOND transcript entry. The user sees
the message twice (structured card + raw-header copy).
FIX: exactly ONE inbound transcript entry. The plugin must receive the message for
the MODEL's context WITHOUT producing a duplicate line — e.g. the structured upsert
IS the model's message (tag it so the poll loop does not re-store), or
`chatbackend_send` is given an "already-stored inbound" mode that feeds the model
but does not re-emit. Single write, single render.

**B2 — non-canonical marker.** The model header is `format!("[CodeTerm inbound {}]")`
(messenger.rs:353) — square brackets, NOT the project-canonical
`-=-codeterm:<kind> k=v…-=-payload` from `message_markers.rs` (OPEN=`-=-codeterm:`,
CLOSE=`-=-`). Every other channel uses the canonical marker; this one drifted.
FIX: build the inbound header via `message_markers::Marker` (kind e.g. `inbound`
or `pane`, attrs from_label/from_pane/from_model/from_mesh) so it's consistent and
round-trips through the same parser (`message_markers::parse` + chat-core/markers.ts).

**B3 — reply loses sender prefix.** When the chatBackend model replies via the
codeterm tool (`codeterm send --pane <from_pane>`), the recipient's PTY gets the
reply with NO `-=-codeterm:pane from=…-=-` prefix — the codeterm-tool exec
subprocess doesn't carry the chatBackend pane's identity (`$CODETERM_PANE_ID`), so
the daemon can't stamp the sender. The return path is asymmetric.
FIX: the chatBackend pane's tool exec (where host runs the curated `codeterm`/`exec`
tools) must run with the pane's identity in env (CODETERM_PANE_ID=<chatBackend pane>
+ workspace + socket), so `codeterm send` from the tool is stamped "from
<chatBackend label>" by Messenger exactly like any pane. Investigate the
chatBackend tool-exec path (host.exec / the ReAct loop's exec) and thread the
pane identity. (Symmetry: PTY→chatBackend already carries identity after M1; this
makes chatBackend→PTY carry it too.)

**Design principle (solid gateway):** Messenger.deliver is the SINGLE chokepoint;
chatBackend inbound = exactly ONE entry carrying BOTH structured sender fields
(render) AND the canonical marker (model); chatBackend→anything replies carry the
pane identity so routing is symmetric. Canonical marker, single-write, threaded
identity — no band-aids.

**TDD (mandatory):**
- deliver to a chatBackend pane → transcript gains EXACTLY ONE inbound entry
  (assert count == 1, not 2), with structured sender fields AND a canonical
  `-=-codeterm:…-=-` marker (assert NO `[CodeTerm inbound]`).
- the inbound marker round-trips through `message_markers::parse`.
- a reply from a chatBackend pane's codeterm-tool `send` to a PTY pane → the PTY
  recipient's injected text carries `-=-codeterm:pane from=<chatBackend label>
  pane=<id>…-=-` (assert prefix present + correct sender).
- existing PTY-path delivery tests stay byte-identical.

Track: **WM.M1b** (codex, alpha backend; + the chatBackend tool-exec identity may
touch the exec/ReAct path). Gate: cargo build/test (messenger + pane_chat) + hook
clippy src+cli. Run AFTER the alpha checkout is free.

<!-- revision: 2 -->
