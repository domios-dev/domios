# Unified Message Markers — Implementation Plan

<!-- revision: 1 -->

Spec: `docs/superpowers/specs/2026-06-08-message-markers-design.md`
Branch: `feat/system-plugins-intrinsic` (commit separately as `refactor(markers)`)

Format: `[[ct:<kind> k=v …]]<payload>`. Sentinel = single constant. Bottom-up:
core builder/parser (both langs, parity-tested) → migrate producers → migrate
consumers → update legacy-format tests → verify.

## Stage 1 — Rust core (`src-tauri/src/message_markers.rs` + tests)

- `const NS = "ct"`, `OPEN = "[[ct:"`, `CLOSE = "]]"`.
- `Marker { kind, attrs: Vec<(String,String)>, payload }` builder: `new/attr/
  payload/build`. `attr` percent-encodes value (space, `]]`, `%`, `"`, newline).
- `parse(&str) -> Option<ParsedMarker { kind, attrs: HashMap, payload }>`:
  return None unless it starts with `OPEN`; find the matching `]]`; split
  `kind` + `k=v` tokens; percent-decode values; rest = payload.
- Convenience: `voice(file, transcript: Option<&str>, note: Option<&str>)`,
  `photo(file, name)`, `attachment(file)`, `telegram(user, msg: Option<&str>)`,
  `mesh(peer, pane)`, `pane(label, pane)`, `report(from)`, `hook(json)`.
- Register `pub mod message_markers;` in `lib.rs`.
- Tests: round-trips per kind; encoding of spaces/`]]`/`%`/quotes/newline;
  `parse` None on plain text and on `[[ct:` without a close; payload containing
  `]]`. Define `PARITY: &[(&str /*build*/, &str /*wire*/)]` fixture array.

## Stage 2 — TS core (`packages/chat-core/src/markers.ts` + test)

- Mirror: `OPEN`/`CLOSE` consts, `buildMarker(kind, attrs, payload)`,
  `parseMarker(s): { kind, attrs, payload } | null`, typed helpers matching the
  Rust convenience constructors.
- Export from `packages/chat-core/src/index.ts`.
- `markers.test.ts`: the SAME parity fixture table as Rust (copy the array);
  round-trip + plain-text-null + encoding cases. This is the cross-lang guard.

## Stage 3 — Migrate Rust producers

- `bridge/voice_transcribe.rs::format_attachment_line` → `Marker::voice/photo/
  attachment` (file attr + transcript-as-payload or note attr). Keep the
  no-daemon / unavailable wording in `note`.
- `bridge/manager/mod.rs`: telegram/mesh source prefix (the
  `format!("[{}: {}{}] {}", …)` site) → `Marker::telegram/mesh` with payload =
  body; hook metadata → `Marker::hook`.
- `api/pane_routes.rs` + `socket/server/pane.rs`: `[from {} pane={}]` →
  `Marker::pane(label, pane)`.
- `service/hierarchy.rs`: `[Report from {}]` → `Marker::report(from)`.
- Mesh prefix producer (wherever `[Mesh: …]` is built — grep) → `Marker::mesh`.

## Stage 4 — Migrate TS consumers

- `src/lib/chatPrefixes.ts`: drop `PANE_RE`/`BRIDGE_RE`; call `parseMarker()`;
  map `ct:telegram`→telegram, `ct:mesh`→mesh, `ct:pane`→pane, `ct:report`→
  service/report origin. Preserve the returned `{ origin, body }` shape.
- `packages/chat-ui/src/MessageBubble.tsx`: replace `parseVoiceMessage` body and
  the `\x00«file»`/`[attached]` `attachRe` with `parseMarker()` for
  `ct:voice|photo|file`; render the existing `VoiceCard` / file badge from it.

## Stage 5 — Update legacy-format tests

Grep + fix every test asserting an old literal: `bridge/manager/tests.rs`
(voice/telegram bodies), `service/inject_verify.rs` (`[Mesh: …]` expected
strings), any `chatPrefixes` test. Re-assert against the new marker shape.

## Stage 6 — Verify

- `cargo test message_markers bridge:: inject_verify pane`, `./build.sh check`,
  `pnpm -s exec tsc --noEmit`, chat-core test run.
- Live: restart dev; Playwright screenshot the opencode chat — voice card +
  origin chips still render from the new markers; send across providers, 0 errors.
- Docs: note the marker format in `docs/PLUGINS.md` or a short `docs/contributing`
  note; bump revision where touched.

## Status (2026-06-08)

- **Done & verified**: Stage 1 (Rust core `message_markers.rs`, 7 tests), Stage 2
  (TS core `chat-core/markers.ts`, parity-checked against the Rust fixture), and
  the **content markers** — voice/photo/file: producer (`voice_transcribe`),
  consumer (`MessageBubble` voice card + file badge), tests updated (bridge 86
  green, tsc 0). Voice notes now build `[[ct:voice file=… status=…]]payload`,
  submit to opencode (no leading `/`), and render as the voice card.
- **Pending — Stage 3/4 source prefixes** (telegram/mesh/pane/report/hook):
  deliberately deferred. These have several entangled formats (the generic
  bridge prefix in `manager`, `[from … pane=…]` in pane_routes/socket, the
  distinct `[Mesh: peer pane=…]` in inject_verify) and change agent-facing
  cross-source context — worth a focused, separately-verified pass (mesh +
  telegram round-trips) rather than rushing. The core + `chatPrefixes` consumer
  pattern (try `parseMarker` first, map kind→ChatOrigin, keep slash logic) is
  ready to apply.

## Rollback

New module + format swap. Revert the commit. No stored-state migration (old
transcripts just render legacy-unparsed).
