# Structured Chat Source — Implementation Plan

<!-- revision: 1 -->

Spec: `docs/superpowers/specs/2026-06-07-structured-chat-source-design.md`
Branch: `feat/system-plugins-intrinsic` (continues; commit separately as `feat(chat)`)

Verified against the real `~/.local/share/opencode/opencode.db`:
`message(id, session_id, time_created, data{role,time})` +
`part(id, message_id, session_id, time_created, data{type,...})`. Part types:
`text` (`.text`), `reasoning` (`.text` → thinking), `tool` (`.tool`, `.state`),
plus step-start/step-finish/compaction/patch/file (skipped in v1). `time_created`
is epoch ms → the cursor.

## Stage 1 — Wire types (`agent/chat_types.rs`)

Add `ChatMsg` (camelCase serde, fields: id, type→`kind`, content, timestamp,
toolName) and `StructuredChat { messages: Vec<ChatMsg>, cursor: Option<String> }`,
both `Serialize` (+ `Deserialize` for tests).

## Stage 2 — Provider contract (`agent/provider.rs`)

Defaulted trait methods: `uses_structured_chat() -> bool { false }` and
`read_structured_chat(&self, cwd, session_id, cursor: Option<&str>) ->
StructuredChat { default }`.

## Stage 3 — opencode reader (`plugin/agent_opencode/provider.rs`)

- `uses_structured_chat() -> true`.
- `read_structured_chat`: one read-only sqlite query
  `SELECT p.id, p.time_created, p.data, m.data FROM part p JOIN message m ON
  m.id = p.message_id WHERE p.session_id = ?1 AND p.time_created >= ?2 ORDER BY
  p.time_created ASC, p.id ASC` (cursor ms, default 0). Map each part by type:
  text→(user|assistant by role), reasoning→thinking, tool→tool_call
  (toolName=`tool`, content=status/output summary); skip others. `id` = part id
  (stable), `timestamp` = part time_created. cursor = max time_created seen.
- Private helper `part_to_chat_msg(part_data, role, id, ts) -> Option<ChatMsg>`
  unit-tested.

## Stage 4 — Structured poller (`service/pane_chat/`)

- `structured_poller.rs`: `spawn_structured_poller(cfg, registry, sink) ->
  TailerHandle` (reuses `TailerHandle::new`). Thread loop: every
  `poll_interval`, `read_structured_chat(cursor)`; if messages, `sink.emit(pane,
  messages, cursor)`; advance cursor; exit on stop flag. Config carries
  pane_id/provider_id/cwd/session_id/start_cursor/poll_interval.
- `structured_sink.rs`: `StructuredSink` trait + `TauriStructuredSink` emitting
  `pane-chat-msg-{pane}` via `emit_everywhere` (web mirror gets it too).
- Export from `mod.rs`.

## Stage 5 — Host wiring (`commands/pane.rs`)

`ChatStartResult`: add optional `messages: Option<Vec<ChatMsg>>`,
`cursor: Option<String>`. In `chat_start_core`, after session resolution, if
`uses_structured_chat`: read initial structured chat, return messages+cursor
(empty text), spawn the structured poller into `chat_registry` (skip when
`acquire` shares an existing worker). Else: current JSONL path. `chat_stop`
unchanged (registry stops either worker via the shared `TailerHandle`).

## Stage 6 — Web (`components/Terminal/PaneSessionChat.tsx`)

`ChatStartResult` type gains `messages?`, `cursor?`. On start, if `res.messages`
defined → structured mode: seed a `Map<id, ChatMessage>`, listen
`pane-chat-msg-{pane}` merging by id, `parsed = [...map.values()]` sorted by
timestamp. Else current raw-text mode. Downstream (dedupe/group/MessageBubble)
unchanged.

## Stage 7 — Verify

- Rust: opencode reader against a temp fixture db (build the two tables, insert
  message+parts, assert mapping/cursor/`>=` re-read); `uses_structured_chat`
  flags; `chat_start_core` routing (guarded — needs a provider stub).
- `./build.sh check`, `cargo test pane_chat`, `cargo test agent_opencode`, tsc.
- Manual: open the real opencode pane, toggle chat, confirm history renders and
  a new turn streams in.
- Docs: extend `docs/PLUGINS.md` "session source" note; bump.

## Rollback

Additive (defaulted trait methods, optional result fields, new modules, one web
branch). Revert the commit; JSONL providers untouched.
