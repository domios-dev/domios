# Structured (DB-backed) Chat Source — Design

<!-- revision: 1 -->

## Problem

The chat-tail pipeline assumes a provider stores its session as a single
append-only JSONL file: `chat_start_core` resolves `session_jsonl_path()`, reads
the tail bytes, and spawns a 500 ms byte-offset tailer that emits raw text on
`pane-chat-raw-{pane}`; the **browser** parses that text
(`parseClaudeJsonlSession` / `parseCodexJsonlSession`).

opencode is SQLite-backed (`opencode.db` + `message`/`part` rows). It has no
tailable JSONL file, so `session_jsonl_path()` returns `None` and
`chat_start_core` dead-ends at `"provider has no JSONL"` (`commands/pane.rs:322`)
— opencode shows no chat, only raw xterm. The Rust side already reads its store
(`agent_opencode::provider::db_messages` / `parse_session_messages`, read-only
`SQLITE_OPEN_READ_ONLY | NO_MUTEX`) for search indexing.

Goal: give the provider contract a second session-source form — **structured
messages parsed in Rust** — so opencode (and any future DB-backed provider) can
serve chat, reusing the existing per-pane chat lifecycle and the shared
`MessageBubble` renderer. No rewrite of the working JSONL path.

## Non-goals

- Not migrating claude/codex to Rust-side parsing — the hybrid
  (file→browser-parse for JSONL providers, DB→Rust-parse for structured) is
  intentional. A future unification is noted, not done.
- No tool-call/diff fidelity beyond text + role for opencode v1; the wire shape
  carries the fields, opencode fills what it can read now (text parts), richer
  blocks later.
- No new DB writes anywhere; opencode's store stays read-only.

## Design

### Provider contract (Rust, `agent/provider.rs`)

Two defaulted trait methods, so existing providers are unaffected:

```rust
/// Provider serves chat as structured messages it parses itself (e.g.
/// sqlite) rather than a tailable JSONL file. When true, the host uses
/// read_structured_chat instead of session_jsonl_path.
fn uses_structured_chat(&self) -> bool { false }

/// Read structured chat for a session. `cursor` is an opaque bookmark
/// from a prior call (None = from the start). Returns new-or-updated
/// messages plus the next cursor. Mirrors the JSONL tail: only the
/// changed tail need be returned, but returning more is safe — the UI
/// merges by message id.
fn read_structured_chat(
    &self,
    _cwd: &Path,
    _session_id: &str,
    _cursor: Option<&str>,
) -> StructuredChat {
    StructuredChat::default()
}
```

New wire type (`agent/chat_types.rs`), shaped to the frontend `ChatMessage`
(`NormalizedChatMessage`) so the browser renders it with **no extra mapping**:

```rust
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMsg {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,           // "user" | "assistant" | "thinking" | "tool_call" | ...
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    // room to grow (toolInput, toolResult, ...) without a contract bump
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StructuredChat {
    pub messages: Vec<ChatMsg>,
    pub cursor: Option<String>,
}
```

The `id` must be stable across polls (opencode message id from the blob) so the
UI can replace a growing in-progress message rather than duplicate it.

### opencode implementation (`plugin/agent_opencode/provider.rs`)

- `uses_structured_chat() -> true`.
- `read_structured_chat(cwd, session_id, cursor)`: reuse the existing
  read-only DB access. Query messages for the session ordered by
  `time_created`; when `cursor` is set, fetch `time_created >= cursor` (note
  `>=`, not `>`) so the latest, still-streaming message is re-read and its
  growing text is picked up. Map each message: role → `kind`
  (`user`/`assistant`/`system`), concatenated text parts → `content`, message
  id → `id`, `time_created` → `timestamp`. `cursor` = max `time_created` seen.
  Factor the row→ChatMsg mapping so `parse_session_messages` (search) and this
  can share the part-concatenation logic (DRY).

### Host wiring (`commands/pane.rs`, `service/pane_chat/`)

`chat_start_core`, after resolving `(provider, cwd, session_id)` and the
session-roll check, branches:

- **Structured provider** (`uses_structured_chat`): call
  `read_structured_chat(cwd, session_id, None)`; return `ChatStartResult` with
  `messages: Some(delta.messages)` and `cursor: delta.cursor` (and empty
  `text`). Spawn a **structured poller** (new `spawn_structured_poller`,
  sibling of `spawn_raw_tailer`) keyed in the same `chat_registry`; every 500 ms
  it calls `read_structured_chat(cwd, session_id, last_cursor)` and, when there
  are messages, emits `{messages, cursor}` on `pane-chat-msg-{pane}`. Lifecycle
  is identical to the file tailer — created on start, killed by
  `chat_registry.stop` on `chat_stop` (and on drop). No work when chat is
  closed.
- **JSONL provider**: unchanged path.

`ChatStartResult` gains:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub messages: Option<Vec<ChatMsg>>,
#[serde(skip_serializing_if = "Option::is_none")]
pub cursor: Option<String>,
```

The structured poller is its own small module mirroring `tailer.rs`'s
stop-flag/handle contract, so `ChatRegistry` stores either handle behind the
same stop semantics (a thin enum or a boxed `ChatWorker` trait with `stop()`).

### Web (`components/Terminal/PaneSessionChat.tsx`)

- `ChatStartResult` type gains `messages?: ChatMessage[]`, `cursor?: string`.
- On start, if `res.messages` is present → **structured mode**: seed a
  `Map<id, ChatMessage>` from it, and `listen("pane-chat-msg-{pane}")` merging
  each delta message by `id` (replace-or-insert, preserving order by first-seen
  / timestamp). `parsed = Array.from(map.values())`.
- Else → current raw-text mode (`parseClaudeJsonlSession` etc.).
- Everything downstream (`dedupeRecentUserMessages`, `groupToolResults`,
  `MessageBubble`) is unchanged — structured messages are already
  `ChatMessage`-shaped.

### Load characteristics

The poller is a single read-only, session-scoped indexed query
(`WHERE session_id = ? AND time_created >= cursor`) every 500 ms, only while the
chat panel is open for that pane (same cadence and lifecycle as the JSONL
tailer). Read-only `NO_MUTEX` open does not block opencode's own writes (WAL).
Closed chat → no poller → zero load.

## Error handling

- `read_structured_chat` failures (DB missing, locked, parse error) → return an
  empty `StructuredChat`; the poller treats it as "nothing new" and retries,
  exactly like the file tailer tolerating a not-yet-created session file.
- A structured provider that returns no messages at start still returns
  `messages: Some(vec![])` (not `None`) so the web enters structured mode and
  waits on deltas, rather than falling back to raw text.

## Testing

Rust:
- opencode `read_structured_chat` against a temp `opencode.db` fixture: maps
  roles, concatenates parts, stable ids; cursor advances; `>=` re-read picks up
  an appended part to the last message without duplicating earlier ones.
- `uses_structured_chat` true for opencode, false for claude/codex (default).
- `chat_start_core` routes a structured provider to `messages`/`cursor` and a
  JSONL provider to `text` (existing behaviour intact).
- shared row→ChatMsg/part-concat helper unit-tested once.

Web: type-check; the structured branch is small and covered by manual run.

Manual: open an opencode pane with history, toggle chat → messages render;
send a turn → new message streams in via the poller.

## Risks / mitigations

- **In-progress message duplication** — solved by stable id + `>=` cursor
  re-read + merge-by-id on the web.
- **Two parsing worlds** (browser JSONL vs Rust structured) — accepted; the
  seam is the `ChatMsg` wire type, identical to what the browser parsers already
  emit, so the renderer is shared. Unification is a later option.
- **Contract churn** — additive: new defaulted trait methods, new optional
  `ChatStartResult` fields; JSONL providers and the REST/web-mirror path are
  untouched.
