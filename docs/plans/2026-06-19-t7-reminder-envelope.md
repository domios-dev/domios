# T7 — Reminder envelope + chat message meta

> Independent track (disjoint from T6). Reminders queued via `codeterm send --delay`/`--at`
> must arrive through the SAME `-=-codeterm:…-=-` envelope as report/pane/mesh, as a
> `reminder` kind carrying who-set-it + when-set + fire-time, rendered as a pretty card
> in Chat UI. Persist the metadata so it shows both when queued and when delivered.
> Bonus (careful): surface per-message JSONL timestamps in the chat bubble.

## Why
Today `deliver_claimed` (`src-tauri/src/delayed_send.rs:370-384`) injects `msg.text` RAW
via `send_text_when_ready` — bypassing the envelope builder. So a fired reminder lands as
naked text (`[self-reminder] …`) with no provenance. We want it framed + rendered like
the other injected-message kinds, with metadata visible.

## Seams (from recon, file:line)
- Envelope builder: `src-tauri/src/service/msg_prefix.rs:44-109` (`CtxPrefix`), kinds enum
  `CtxKind` at `msg_prefix.rs:10-16` (today `Message`, `Report`). Generic marker infra:
  `src-tauri/src/message_markers.rs:59-104`.
- Delayed queue: `src-tauri/src/delayed_send.rs:14-94` (`DelayedMessage`), delivery
  `deliver_claimed` at `:370-384`, scheduler `:348-368`. Persisted to `delayed_send.json`.
- API/CLI: `src-tauri/src/api/delayed_send_routes.rs:29-86`, `cli/src/cmd/delayed.rs`.
- TS parser: `src/lib/chatPrefixes.ts:30-93` (`detectTransportPrefix`); marker core
  `packages/chat-core/src/markers.ts:41-70`; `ChatOrigin` type
  `packages/chat-core/src/types.ts:69-87`.
- Card renderer: `packages/chat-ui/src/codeterm/CodetermCard.tsx:12-59`; single bubble
  `packages/chat-ui/src/MessageBubble.tsx`.
- Chat message shape: `src-tauri/src/agent/chat_types.rs:126-167` (`ChatMsg`, has
  `timestamp: Option<String>`), TS mirror `packages/chat-core/src/types.ts:40-62`.

## Scope

### A. Persist who-set-it on the queued reminder (backend)
- Add to `DelayedMessage` (delayed_send.rs:14): `created_by_pane: Option<String>` and
  `created_by_label: Option<String>` (sender's display name/model if resolvable). Bump the
  `delayed_send.json` version and load defaults for old rows (None) — hand-migration OK.
- Capture creator at enqueue: the API route + CLI `send --delay` must pass the CALLER's
  pane id (the CLI runs inside a pane → `$CODETERM_PANE_ID`). Thread it into
  `DelayedMessage`. Resolve a label/model from that pane if available (reuse whatever the
  report/pane envelope uses to get label+model).

### B. Reminder envelope on delivery (backend)
- Add `CtxKind::Reminder` (msg_prefix.rs:10) + a `CtxPrefix::reminder(...)` constructor that
  emits attrs: `from`/`label` (who set), `pane` (creator pane), `model` (if known),
  `set_at` (created_at_ms), `fire_at` (scheduled_at_ms), `id` (the dmsg id). Use the
  existing percent-encoding + key=value assembly — do NOT hand-roll a new format.
- In `deliver_claimed`, build the reminder prefix from the `DelayedMessage` metadata and
  wrap `msg.text` before `send_text_when_ready` (mirror how report/pane delivery wraps).
- If `reminder` needs registering in `message_markers.rs` kind registry, do it.

### C. Pretty reminder card (frontend)
- Extend `ChatOrigin` (chat-core/types.ts:69) with a `reminder` variant
  `{ kind:"reminder"; whoSet?:string; pane?:string; model?:string; setAt?:string; fireAt?:string; id?:string }`.
- `detectTransportPrefix` (chatPrefixes.ts) recognizes `marker.kind === "reminder"` →
  returns that origin + payload body (percent-decoded).
- Render in `CodetermCard.tsx`: a clock/bell card — title "Reminder" + "from {whoSet}" when
  known; subline "set {relative(setAt)} · fired {relative(fireAt)}" (humanize; show absolute
  on hover/title). Keep it consistent with existing card styling and theme vars; no new
  heavy deps (reuse lucide icons already in use).
- Add a parser unit test (a `-=-codeterm:reminder …-=-` sample → correct origin+body) and a
  light CodetermCard render test for the reminder kind.

### D. (Bonus, careful) per-message timestamp in the bubble
- `ChatMsg.timestamp` already flows to TS as `timestamp?: string`. In `MessageBubble.tsx`
  add a SUBTLE timestamp affordance (muted, tiny, e.g. on hover or as a small footer) using
  `var(--ct-muted-fg)`; do NOT clutter every bubble or change layout density noticeably.
  Gate behind something unobtrusive (hover reveal is ideal). This is secondary — land A–C
  first; only do D if it stays clean.

## Discipline
- Own `pnpm install --frozen-lockfile`; NEVER symlink node_modules. The chat-core/chat-ui
  are workspace packages — rebuild them if their `dist` is consumed (`pnpm build` or the
  package build script) so the app picks up the type/renderer changes.
- Self-gate ALL green: `pnpm exec tsc --noEmit`; `pnpm test:pw:lint`; `./build.sh check`;
  `./build.sh test`; pre-push clippy BOTH manifests via the curated `CLIPPY_ALLOW` array
  (`eval "$(sed -n '/^CLIPPY_ALLOW=(/,/^)/p' .githooks/pre-push)"`); and `cargo fmt --check`
  (both src-tauri and cli) — fmt is part of the push hook, do not skip it.
- Acceptance: queue a reminder (`codeterm send --pane <self> --delay 30s "hi"`), let it fire,
  confirm the delivered text is wrapped `-=-codeterm:reminder from=… pane=… set_at=… fire_at=… id=…-=-`
  (not raw), and that the parser yields a `reminder` origin. Component test proves the card.
- Commit on your worktree branch only; no push/merge; no co-author/no "Generated with".
  When done: `codeterm agent report` with hash, files, gate results, acceptance, deviations.

<!-- revision: 1 -->
