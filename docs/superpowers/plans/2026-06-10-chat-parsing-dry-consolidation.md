# Chat Parsing DRY Consolidation — Audit & Plan

> **STATUS (2026-06-10): IMPLEMENTED (Direction A).** The live path is now a
> single parser: host `plugin.js parseSessionDelta` → Rust `rich_to_chat_msgs`
> → normalized `ChatMsg`, rendered identically by desktop and web for all three
> providers. Eval coverage: `rich_flatten_parity_block_derivable_kinds` and
> `rich_flatten_parity_system_meta_local_command` in
> `src-tauri/src/plugin/js_runtime/parity_test.rs` (golden fixtures, both green).
> Verified live: the cross-pane Playwright test renders real claude chat through
> the rewired desktop path. The TS `parseSession`/`parseLine` plugin fields and
> their contract type are removed; the desktop no longer parses JSONL.
> **Remaining (mechanical follow-up):** physically delete the now-unreferenced
> `plugins/provider-{claude,codex}/src/parser.ts` parse functions + their
> `parser.test.ts`, migrate `renderer.tsx`'s `ChatMessage` type import to
> `@codeterm/chat-core`'s `NormalizedChatMessage`, and drop the now-dead
> `makeLineParser`/`messagesToEvents` in `src/lib/plugins/messageEvents.ts` and
> the `raw_text_sink.rs` / `spawn_raw_tailer` Rust path.



> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development or
> superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. **Do not start the
> Phase 2 rewrite without a green guard** — the cross-pane Playwright test
> (`tests/playwright/suites/03-agents/cross-pane-chat.spec.ts`) plus a new per-provider
> snapshot parity test are the safety net for this high-blast-radius change.

**Goal:** Collapse the per-provider chat-message parsing onto ONE parser and ONE normalized
on-the-wire shape, consumed identically by desktop (Tauri) and web (REST), for all three
providers (claude / codex / opencode).

**Architecture:** Host-authoritative normalization. The QuickJS provider plugin
(`plugins/provider-*/plugin.js`) is the single parser (already the authoritative surface per
the provider-plugin migration). The Rust layer flattens its output to `ChatMsg` and both
ships it in the `chat_start` snapshot and streams `ChatMsg` deltas over one event. The
desktop drops its duplicate TS parser and raw-text tailer.

**Tech stack:** Rust (`src-tauri`), QuickJS plugin host, React/TS (`packages/chat-*`,
`src/components/Terminal/PaneSessionChat.tsx`).

---

## Audit — current state (the DRY violation, exactly)

There are **two hand-maintained parsers for the same claude/codex JSONL format**, plus a
third structured path for opencode, feeding **three** client streaming channels.

### Parsers
| # | Source | Output | Runs in | Feeds |
|---|--------|--------|---------|-------|
| 1 | `plugins/provider-claude/plugin.js::parseSessionDelta` (L323, 744-line file) | RichMessage (role + typed blocks) | QuickJS host (called by Rust) | REST `/chat` snapshot + JSONL tailer |
| 2 | `plugins/provider-claude/src/parser.ts::parseJsonlSession` (542 lines) | `NormalizedChatMessage[]` | Desktop bundle (vite) | Tauri `chat_start` raw-text path |
| 3 | `agent_opencode::read_structured_chat` (sqlite) | `ChatMsg[]` | Rust | both desktop + web |

**#1 and #2 are independent re-implementations of the same format.** `plugin.js` is NOT
built from `src/parser.ts` (no build step links them) — they drift independently. Codex has
the same shape (native `parseSessionDelta` / its plugin parser vs the TS parser).

### Snapshot paths
- **claude/codex desktop:** `chat_start` → `ChatStartResult { text: raw JSONL }` →
  `getPlugin(provider).parseSession(rawText)` in TS (parser #2). — `PaneSessionChat.tsx:531`
- **claude/codex web/REST:** `GET /api/panes/{id}/chat` → `pane_chat_snapshot` →
  `parse_session_delta` (parser #1) → `rich_to_chat_msgs` → `ChatMsg[]`. —
  `src-tauri/src/service/pane_chat/mod.rs:66-111`
- **opencode (both):** `read_structured_chat` → `ChatMsg[]` directly.

### Streaming sinks (three channels)
| Event | Payload | Sink | Used by |
|-------|---------|------|---------|
| `pane-chat-raw-{id}` | `{ text }` | `raw_text_sink.rs` | claude/codex **desktop** (accumulate → re-parse #2) |
| `pane-chat-msg-{id}` | `{ messages: ChatMsg[], cursor }` | `structured_sink.rs` | opencode (mergeById) |
| `pane-chat-{id}` | `{ messages: RichMessage[] }` | `tauri_sink.rs` (`TauriDeltaSink`) | **appears unused by the desktop chat effect** — verify & delete |

### Client consume — `PaneSessionChat.tsx:464-532`
- `res.messages` present → structured path (`pane-chat-msg-{id}` + `mergeById`).
- else → raw path (`pane-chat-raw-{id}` accumulate) + `getPlugin().parseSession(rawText)`.
- Origin/marker handling (`splitChatPrefixedMessages`, `parseMarker`) runs AFTER parsing on
  the resulting messages — provider-independent, stays as-is.

### Type shapes
- Rust `ChatMsg` — `agent/chat_types.rs:57` — `{id, kind, content, timestamp, tool_name,
  tool_input, tool_result}`.
- TS `NormalizedChatMessage` — `packages/chat-core/src/types.ts:21` — superset:
  `+ attachments?, provider?, subtype?`.
- **Parity gap:** `ChatMsg` lacks `attachments` / `subtype` / `provider`. Any consolidation
  onto `ChatMsg` MUST add these (or prove the desktop renderer never needs them) or image
  attachments + system-message subtypes regress.

---

## Decision required before Phase 2

Direction A (recommended): **host-authoritative** — keep parser #1 (`plugin.js`), delete #2
(`src/parser.ts` desktop parser), desktop renders `ChatMsg`. Aligns with the provider-plugin
migration (plugins authoritative). Cost: extend `ChatMsg` + `rich_to_chat_msgs` for parity.

Direction B: client-authoritative — everything ships raw and TS parses. Rejected: opencode
is sqlite-backed with no raw JSONL; would force a reverse port.

**This is an architecture call tied to the #45 plugin migration — confirm Direction A with
the maintainer before Phase 2.**

---

## Phase 0 — Guards (do first, no production change)

- [ ] **Confirm `pane-chat-{id}` (RichMessage `TauriDeltaSink`) is dead** for the desktop
  chat effect. Grep `pane-chat-${` in `src/`. If unused, note it for deletion in Phase 3.
- [ ] **Add a per-provider snapshot parity test.** For a fixed JSONL fixture, assert
  `pane_chat_snapshot` (parser #1 → ChatMsg) and `parseJsonlSession` (parser #2 →
  NormalizedChatMessage) produce the SAME logical messages (same kinds, content, tool
  names/inputs, attachments). This test is what proves Phase 2 doesn't regress. Put fixtures
  under `src-tauri/tests/fixtures/` + a TS test in `plugins/provider-claude/tests/`.
- [ ] Run the existing cross-pane Playwright test green to capture the baseline:
  `./node_modules/.bin/playwright test -c tests/playwright -g "two chat panes exchange"`.

## Phase 1 — Parity-extend `ChatMsg`

- [ ] Add optional `attachments`, `subtype`, `provider` to Rust `ChatMsg`
  (`agent/chat_types.rs`) with `#[serde(skip_serializing_if = "Option::is_none")]`.
- [ ] Teach `rich_to_chat_msgs` (`service/pane_chat/mod.rs:33`) + `plugin.js::parseSessionDelta`
  to populate them (image blocks → attachments; system/summary lines → subtype).
- [ ] Extend the Phase-0 parity test to cover the new fields. Green before continuing.

## Phase 2 — Unify snapshot + stream onto ChatMsg

- [ ] `chat_start` / `chat_start_core` (`commands/pane.rs`): for JSONL providers, return
  `messages: Some(pane_chat_snapshot(...))` instead of raw `text`. Keep `text`/offsets only
  for the "load older" window.
- [ ] Streaming: add a ChatMsg-emitting tailer — wrap the RichMessage delta from the tailer
  through `rich_to_chat_msgs` and emit on `pane-chat-msg-{id}` (reuse `structured_sink`).
  Retire `spawn_raw_tailer` for chat (keep raw tailer only if "load older" still needs it).
- [ ] `PaneSessionChat.tsx`: delete the `pane-chat-raw-{id}` branch and the
  `getPlugin().parseSession(rawText)` re-parse. All providers → `res.messages` +
  `pane-chat-msg-{id}` + `mergeById`. Keep `splitChatPrefixedMessages`/`parseMarker` on the
  ChatMsg content.
- [ ] Verify: cross-pane Playwright test green; manual 3-provider chat smoke (claude/codex/
  opencode) renders text, thinking, tool calls, tool results, images, origin headers.

## Phase 3 — Delete the duplicate parser + dead channels

- [ ] Delete `plugins/provider-claude/src/parser.ts::parseJsonlSession` desktop usage and the
  `parseSession` plugin field once nothing calls it (check `src/components/ChatView/parser.ts`
  shim + `makeLineParser`). Same for codex.
- [ ] Delete `raw_text_sink.rs` + `pane-chat-raw-*` plumbing if unused post-Phase-2.
- [ ] Delete `tauri_sink.rs` (`TauriDeltaSink`, `pane-chat-{id}`) if confirmed dead in Phase 0.
- [ ] Update `docs/PROMPTS.md` / architecture docs + bump revision tags.

## Risk notes
- No existing eval covers chat rendering fidelity beyond the new cross-pane test — Phase 0's
  parity test is mandatory, not optional.
- `mergeById` semantics differ from raw-append: ensure JSONL deltas carry STABLE ids across
  polls (the in-progress assistant message must keep its id so it updates in place, not
  duplicates). `rich_to_chat_msgs` ids are `"{base_index}-{block_index}"` — base_index is the
  message ordinal; confirm it's stable across tail polls (offset-relative vs absolute).
- "Load older" backfill (`chat/backfill`) currently re-parses raw text windows — keep it
  working or port it to ChatMsg slices too.

<!-- revision: 1 -->
