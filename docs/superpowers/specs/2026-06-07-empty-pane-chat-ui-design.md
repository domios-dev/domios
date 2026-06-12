# Empty-Pane Chat UI — Design

**Date:** 2026-06-07
**Status:** Approved

## Problem

A freshly spawned agent pane has no JSONL session until the agent's first
response. `chat_start_core` hard-fails on `agent_session_id == None`
("pane has no session_id yet"), and `TerminalPane`'s `onError` handler both
flips the pane back to terminal **and** writes `"terminal"` into the
client-mode store as if the user chose it. Net effect: chat UI cannot be
opened on a fresh pane — not by the new-pane default, not by the explicit
"Open Chat UI" toggle — until one message round-trips. The store poisoning
then makes the terminal mode sticky.

A frontend-only fix (string-matching the error message) was applied earlier
and silently lost in a file rewrite — string matching is too fragile a
contract to survive refactors.

## Decision

Make "no session yet" a **typed, non-error state** of `chat_start`:

1. **Rust — `chat_start_core`** (`src-tauri/src/commands/pane.rs`):
   when the pane has an `agent_type` but no `agent_session_id`, return
   `Ok(ChatStartResult { text: "", start_offset: 0, offset: 0, pending: true })`
   and do **not** install a tailer. Add `pending: bool` to `ChatStartResult`
   (`#[serde(default)]`, always serialized). All other failures (pane not
   found, no agent_type) remain `Err`.
2. **Frontend — `PaneSessionChat.tsx`**: when `res.pending`, render the
   normal empty chat (no spinner, no status line — per user choice).
   The composer already works sessionless (sends ride the PTY via
   `chat_send_text`; optimistic bubbles cover the gap). The
   `chat_start` effect gains `agent_session_id` (from the mux snapshot) as a
   dependency: when the session id appears, the effect re-runs, `chat_start`
   returns real history, and the tailer binds. Successful runs clear any
   stale error.
3. **Frontend — `TerminalPane.tsx`**: `onError` only does
   `setUiMode("terminal")`. It no longer writes the client-mode store —
   an automatic fallback is not a user choice and must not pin future
   decisions.

## Not in scope

- Server-side session watchers (mux snapshot already delivers the id).
- Cleaning previously poisoned sessionStorage entries (per-window session
  state; clears itself; user can toggle manually).
- The "pane has no agent_type" case — the chat toggle/default only exist
  for agent panes, so this stays a genuine error.

## Testing

- Rust unit: pane with `agent_type` but no session → `Ok` with
  `pending == true`, no tailer registered; pane without `agent_type` →
  `Err` (unchanged).
- Manual e2e (desktop + web): new Claude pane opens chat immediately
  (default on), explicit toggle works pre-session, first agent reply
  back-fills history live, genuine errors still fall back to terminal
  without store writes.
