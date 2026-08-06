# `send` false-OK diagnosis (issue #8)

Caller-side guard shipped in `cli/src/cmd/send_text.rs`. The PTY-side root cause
below is **read-only diagnosis** — it lives under the PTY delivery boundary
(`docs/contributing/pty-delivery.md`) and requires explicit approval before any
edit.

## Symptom
`codeterm send "<task charter>" --tab <shell>` returns `OK (sent)` while the
shell in the target tab executes the prose line by line.

## PTY-side root cause (no code edited)
In `src-tauri/src/api/tab_routes/send.rs::send_text`:

- Line ~254 computes `let target_has_agent = state.svc.tab_has_agent(&tab_id);`.
- That value feeds **only** the `prepend(...)` marker decision (whether to inject
  the agent return-address marker). It never gates delivery.
- Every delivery branch (text, `raw`, `batch`, `actions`, mesh) returns
  `Ok(Json(json!({"ok": true, ...})))` once the payload reaches the write queue.
  A `submit_unconfirmed`/`pty_unavailable` classification is the only failure
  shape; "delivered to a shell that will execute it" is not modeled.

So the daemon already knows the target has no agent, but success means "bytes
written to the PTY", identical for an agent that will parse them and a shell
that will run them. The caller cannot distinguish the two from the return value.

## Why the fix is caller-side only
The core PTY invariant is "payload written at most once; never reclassify a
delivered paste as a failure". A refusal must therefore happen **before** the
byte-delivery decision, not inside it. The CLI now probes the target tab record
(`tab.list` → `agent_type` / `backing_kind`, read-only) and refuses task-shaped
prose bound for a raw shell (`TabKind::RawShell`) unless `--to-shell`/`--force`,
returning a non-OK exit(2). No PTY/write-queue/ledger code is touched.

## Suggested PTY-side follow-up (needs Andrey's approval — do not implement)
Differentiate the success envelope by transport/agent, e.g. return
`{"ok": true, "delivered_to": "shell"|"agent"}` from `send_text`, so a
server-side `require_agent` can refuse authoritatively and remote/mesh callers
(where the CLI cannot probe) get the same guarantee. This edits the delivery
return contract and is out of scope for this lane.

<!-- revision: 1 -->
