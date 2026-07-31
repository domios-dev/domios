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
   retry loop, not a recovery path, not a watchdog. The only payload repeat is
   an explicit user-driven Retry, which is a new ledger transaction.
2. **Submission is one Enter, plus a bounded key-only recovery.** The CR is a
   separate write released after paste-ingestion evidence. If acknowledgement
   evidence is missed, only the submit *key* may be re-written, within
   `MAX_SUBMIT_KEY_WRITES`, never the payload — and a retry write may never
   begin with an expired phase deadline (the first CR is the delivery itself
   and always goes out).
3. **Verification classifies; it never gates delivery on heuristics it cannot
   prove.** Screen-echo counting, draft-left-composer, turn transitions are
   *evidence collectors*. Their failure produces `submit_unconfirmed`
   (uncertainty), never a fabricated success and never a zero-write claim.

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

## The Chat send ledger reconciles by exact evidence

`ChatSendLedger` is the authority the UI renders. Its rules:

- `pending → sent` only from evidence; `pending/failed → sent` also happens
  when the tab's **own bound session transcript** appends a user turn whose
  normalized content equals the send's payload
  (`confirm_delivered_user_turn`). One turn confirms at most one send,
  oldest first. Deltas only — history snapshots never feed reconciliation, so
  stale identical turns can never confirm anything.
- Every send records a `SendEvidenceAnchor` at start: provider, cwd, the
  tab's bound session id, and a capability-typed append-window boundary —
  the transcript byte offset for JSONL providers, or the provider's opaque
  `read_structured_chat` cursor for structured stores (`uses_structured_chat`
  capability; a cursor-less non-empty store mints no anchor rather than an
  unbounded window). If Chat was closed during delivery (no live sink), the
  ledger read (`chat_send_status`) runs `scan_transcript_evidence`: it
  inspects ONLY that exact session's append window past the boundary, counts
  exact matching user turns, subtracts turns already claimed by confirmed
  sends of the same tab+session+payload, and confirms oldest-first. Pre-send
  history, another/latest session, and already-consumed turns can never
  confirm.
- Daemon restart: the ledger is in-memory, so the frontend shows a failure
  only while the CURRENT ledger vouches for it — a reconcile response that no
  longer contains a tracked failed send removes the record (no false Retry is
  preserved, and no fabricated `sent` replaces it; the durable transcript is
  the remaining truth).
- Evidence wins races: once a send is `sent` by turn evidence, a late
  transport classification must not downgrade it (`finish` guard).
- A send with no evidence and a terminal classification remains `failed` in
  the internal ledger and logs. Chat UI stays optimistic and never renders
  delivery banners, failure cards, Retry actions, or notifications.

## Hard don'ts

- No sleeps, polling intervals, or timer-driven "success" anywhere in the
  path — deadlines are failure bounds around event waits only.
- No provider-id branches; capability descriptors only.
- No new acknowledgement heuristics on the send path. If evidence quality is
  the problem, improve the *evidence source* (session transcript correlation),
  not the screen-scraping.
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

<!-- revision: 4 -->
