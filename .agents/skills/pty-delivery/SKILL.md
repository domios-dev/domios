---
name: pty-delivery
description: "Review or change Domios PTY input delivery under its evidence rules."
---

# PTY input delivery — the contract

PTY delivery is core infrastructure and it is fragile: every provider TUI
ingests pastes and Enter differently, ConPTY adds its own echo and timing
quirks, and false-positive *failures* after successful delivery are worse than
silent success — they teach users to distrust the product and invite duplicate
sends. This contract exists because the path has been broken repeatedly by
well-meaning "one more heuristic" changes. Read this before touching
`service/pty_io/`, `service/chat_send_transaction.rs`,
`pty/write_queue.rs`, or the delivery ledger.

## The model: optimistic delivery, honest eventual classification

1. **The payload is written at most once.** One bracketed paste to the exact
   PTY the caller resolved. Nothing may ever re-write the payload — not a
   retry loop, not a recovery path, not a watchdog.
2. **Submission uses a bounded key-only schedule.** Write the payload once. A
   provider capability may declare a bounded first-key offset; elapsed time
   triggers an attempt and never claims readiness. After a key, exact normalized
   user-turn evidence in this tab's bound-session append window stops remaining
   keys. Never use screen/composer text as acknowledgement or branch on provider
   identity. Each PTY owns one resettable submit cycle; a newer send cancels the
   previous cycle's remaining keys while preserving its already-queued payload.
   The explicit user-send exception is a bounded durable queue of never-written
   originals: 32 unsettled records and 4 MiB prepared/body bytes per tab, with
   original storage separately bounded. Sends are held only until the current
   launch's exact PTY generation reaches `AgentReady`, then queued sends flush in
   admission order. Admission order applies only to sends held before readiness.
   Every later send is written immediately while the agent is working or an
   earlier cycle is unsettled, cancelling that cycle's remaining keys. If exact
   session binding is missing before that readiness,
   the owner-approved adapter fallback may release a restored head only after two
   distinct same-launch, same-generation ready-idle verdicts at least 0.90, with
   composer readiness at least 0.90 and prompt probability at most 0.10, followed
   by immediate prewrite revalidation. Without an adapter or qualifying evidence,
   the head stays held with a logged pending reason. Readiness latches for an `AgentReady`
   launch; later busy state, missing turn evidence, or unsettled recovery never
   gates another write. Possible handoffs and crashed claims remain held
   uncertainty; no postwrite replay, reopen adoption, or rebind.
3. **Exact turn evidence confirms delivery.** Missing evidence remains
   uncertainty, never a fabricated success or a zero-write claim. Once a paste
   reaches the write queue, its status becomes `written` immediately. Later
   status is `delivered` only on exact own-session user-turn evidence, or
   `submit_unconfirmed` when evidence is absent. Late evidence may upgrade an
   unconfirmed send. Cancelling the remaining submit keys never fails a queued
   payload.

## Classification invariants (`DeliveryError`)

- `pty_unavailable` / `tab_busy_refused` mean **nothing was written**. They may
  only be produced by failures that happen strictly before the paste reaches
  the write queue (no session, preflight refusal, launch gate).
- Once the paste has been handed to the PTY write queue, the transaction may
  only fail as `submit_unconfirmed` (`bytes_written = true`). A submit-key
  write error, a revoked permit, even a PTY that dies after the paste — all of
  these are post-write uncertainty, not "no usable PTY".
- A write-queue **acknowledgement timeout is not a closed PTY**
  (`write_queue::is_write_ack_timeout`). The request is enqueued on the
  serialized writer and normally still lands; claiming `pty_unavailable` here
  is exactly the false-failure incident this contract was written for.

## Durable user admission conserves accepted sends

An admitted USER-initiated send (chat UI, via `start_chat_send`) remains
conserved or honestly classified. Bounded durable admission can refuse capacity,
preparation, or persistence before acceptance; clear the composer immediately and retain the message and attachments on one optimistic
bubble, including rejected or ambiguous RPC completion, without automatic resubmit.
Acceptance requires payload and index durability off the UI/runtime and outside
mux/ledger locks. This user backlog is separate from launch/bootstrap replacement.
The prewrite owner holds only sends accepted before the current launch's readiness
latch or owner-approved restored-session adapter fallback, then writes those held
sends in admission order. A post-latch Chat UI send proceeds directly to its PTY
write even when the provider is busy or another submit cycle is unsettled. Proven
zero-write refusal retains that send as held; possible handoff becomes
`submit_unconfirmed` and never replays. Programmatic agent/mesh/CLI sends keep the
existing honest typed split. A queued record whose exact launch authority is
from an ended launch is settled as `failed` with `launch_ended_before_write`;
its never-written payload is removed and it never enters the next launch.
Claimed or uncertain records remain held and are never replayed.

## The Chat send ledger reconciles by exact evidence

`ChatSendLedger` is the authority the UI renders. Its rules:

- The serialized states are `held`, `written`, `submit_unconfirmed`, `delivered`,
  and `failed`. Transport uncertainty is `submit_unconfirmed` with no user-facing
  error. A user send resolves to `failed` only when its queued, never-written
  launch has ended. Programmatic sends may also fail on a proven pre-write
  refusal.
- `written/submit_unconfirmed → delivered` only happens
  when the tab's **own bound session transcript** appends a user turn whose
  normalized content equals the send's payload
  (`confirm_delivered_user_turn`). One turn confirms at most one send,
  oldest first. Deltas only — history snapshots never feed reconciliation, so
  stale identical turns can never confirm anything.
- Every eligible send records a `SendEvidenceAnchor` at start: provider, cwd,
  the tab's current-launch session identity, and a capability-typed
  append-window boundary. A temporarily unbound file-backed launch may resolve
  its session through the provider's bounded, cwd-scoped session detector.
  Persisted reconciliation always requires the current launch nonce to equal
  the nonce reported by the provider when it bound that session.
  JSONL uses a transcript byte offset. If the exact acknowledged bound
  session's append-only file does not exist yet, its first materialization is
  the boundary; a generic missing path never defaults to offset zero.
  Structured stores use the provider's opaque, exclusive
  `read_structured_chat` cursor
  (`uses_structured_chat` capability; a cursor-less non-empty store mints no
  anchor rather than an unbounded window). If the structured session itself is
  created by the send, its provider-reported creation time must be strictly
  after the captured millisecond before the current binding can resolve that anchor. If
  Chat was closed during delivery, the ledger read (`chat_send_status`) runs
  `scan_transcript_evidence`: it
  inspects ONLY that exact session's append window past the boundary, counts
  exact matching user turns, verifies that nested suffix windows can assign one
  distinct turn to every confirmed same-payload send, and confirms oldest-first. Pre-send
  history, another/latest session, and already-consumed turns can never
  confirm.
- Daemon restart: the ledger is in-memory, so the frontend shows a failure
  only while the CURRENT ledger vouches for it — a reconcile response that no
  longer contains a tracked failed send removes the record (no false Retry is
  preserved, and no fabricated `sent` replaces it; the durable transcript is
  the remaining truth).
- Evidence wins races: once a send is `sent` by turn evidence, a late
  transport classification must not downgrade it (`finish` guard).
- A send with no evidence after the recovery deadline remains `submit_unconfirmed`.
  Anchored sends can still reconcile from later transcript evidence; a send
  whose session or bounded window cannot be identified remains honestly
  live-evidence-only. Chat shows one bubble per send: sending for written or
  unconfirmed input, queued while held, normal when delivered, and a quiet
  not-delivered mark with Retry for proven refusal or failed never-written input.
  Retry reuses the bubble with a fresh correlation token and transaction; it never
  replays uncertain or written input. Provider echoes reconcile into that bubble.

## Hard don'ts

- No sleeps, polling intervals, or timer-driven "success" anywhere in the
  path — deadlines are failure bounds around event waits only.
- No provider-id branches; capability descriptors only.
- Do not add screen/composer acknowledgement, guessed readiness, payload
  retries, or provider-id branches. Exact claimed-session transcript evidence
  classifies a send and may stop its current key schedule; it never gates the
  next post-ready send.
- Do not "fix" a delivery bug by adding a retry that re-writes the payload.
- Launch/bootstrap input never routes through the public post-ready send gate
  (see AGENTS.md "Agent launch delivery").

## Where the seams are

| Concern | Owner |
|---|---|
| Serialized PTY writes + ack | `crates/codeterm-pty/src/pty/write_queue.rs` |
| Paste/Enter transaction + classification | `service/pty_io/submit_transaction.rs` |
| Injection error taxonomy | `service/inject_verify.rs`, `service/delivery_error.rs` |
| Chat send ledger + turn-evidence reconciliation | `service/chat_send_transaction.rs` |
| Closed-chat append-window evidence scan | `service/chat_send_evidence.rs` |
| Live transcript deltas (evidence source) | `service/tab_chat/` sinks |

<!-- revision: 1 -->
