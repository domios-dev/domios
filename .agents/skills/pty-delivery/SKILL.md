---
name: pty-delivery
description: "Review or change CodeTerm PTY input delivery under its owner-approval and evidence rules."
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
   identity. Each PTY owns one resettable cycle: a new send cancels the old cycle
   instead of queueing messages.
3. **Exact turn evidence confirms delivery.** Missing evidence remains
   uncertainty, never a fabricated success or a zero-write claim. Once a paste
   reaches the write queue, an unconfirmed send is `submit_unconfirmed` with
   `bytes_written = true`.

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

## User sends always deliver — never a user-facing refusal

A USER-initiated send (chat UI, via `start_chat_send`) may never surface a
refusal. The wait-then-write path already waits on the observable link-readiness
signals (`await_public_agent_input_ready_blocking`, `await_pty_link_blocking` —
condvar-backed, bounded failure deadlines, no sleeps) before its single write,
so a transient pre-write unlink (booting agent / unlinked PTY) or an unconfirmed
submit degrades to `unknown` (pending-until-link, resolved by later turn
evidence), never `failed`. This is `DeliveryError::surfaces_as_failure(origin)` /
`into_surfaced(origin)`: `SendOrigin::User` always degrades; `Programmatic`
(agent-to-agent / mesh / CLI `send_text`) keeps the honest typed split — a proven
pre-write refusal fails, a post-write submit is unknown. Neutralizing a user
refusal never re-writes the payload: the single write already happened upstream.

## The Chat send ledger reconciles by exact evidence

`ChatSendLedger` is the authority the UI renders. Its rules:

- The serialized states are `accepted`, `unknown`, `confirmed`, and `failed`.
  Transport uncertainty is `unknown` with no user-facing error. A user send never
  resolves to `failed`; only a programmatic send's proven pre-write refusal is
  `failed`.
- `accepted/unknown → confirmed` only happens
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
- A send with no evidence after the recovery deadline remains `unknown`.
  Anchored sends can still reconcile from later transcript evidence; a send
  whose session or bounded window cannot be identified remains honestly
  live-evidence-only. Chat UI never renders delivery banners, failure cards,
  Retry actions, or notifications.

## Hard don'ts

- No sleeps, polling intervals, or timer-driven "success" anywhere in the
  path — deadlines are failure bounds around event waits only.
- No provider-id branches; capability descriptors only.
- Do not add screen/composer acknowledgement, guessed readiness, payload
  retries, or provider-id branches. Use exact claimed-session transcript
  evidence to stop recovery and to reconcile later sends.
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
