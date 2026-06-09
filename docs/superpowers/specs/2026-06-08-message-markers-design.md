# Unified Message Markers — Design

<!-- revision: 1 -->

## Problem

CodeTerm injects several ad-hoc bracket markers into chat messages, produced in
many places and parsed in others, with no shared definition:

**Source prefixes** (parsed into `ChatOrigin` by `src/lib/chatPrefixes.ts`):
- `[from <label> pane=<id>] ` — pane→pane (`api/pane_routes.rs`, `socket/server/pane.rs`)
- `[Telegram: <id> msg_id=<id>] ` / `[Mesh: <peer> pane=<id>] ` — bridge/mesh (`bridge/manager`)
- `[Report from <title>] ` — hierarchy report (`service/hierarchy.rs`)

**Content markers**:
- `[voice] (voice transcript: "...") file://...`, `[photo: name] file://...` (`bridge/voice_transcribe.rs`)
- `\x00«file:...»` / `[attached: ...]` — attachment badge (`chat-ui/MessageBubble.tsx`)
- `[Hook metadata: ...]`, `[callback: ...]`

Every marker is a plain `[...]` token, so a user who literally types `[voice]`
or `[Telegram: x]` is mis-parsed (false positive). The formats are duplicated
across Rust and TS by hand, drift-prone, and hard to extend.

## Goals

- One **collision-resistant, namespaced** marker format for all CodeTerm-injected
  markers, hard to reproduce by accident.
- A single **builder + parser** (Rust and TS, mirrored, parity-tested) so the
  format lives in one place per language and new marker kinds are trivial.
- Migrate every producer and consumer onto it: voice/photo/file attachments,
  telegram/mesh/from-pane/report source prefixes, hook metadata.
- LLM- and PTY-safe: the agent still reads the marker as ordinary text; no
  control bytes; survives bracketed paste and JSONL round-trips.

## Non-goals

- No backward-compat parsing of the old `[...]` formats. Per project policy
  (no external users), the new format replaces them; old session transcripts
  rendering with the old shape is acceptable.
- Not changing WHAT context is conveyed (same origin/attachment info), only the
  envelope.

## Format

```
[[ct:<kind> <k>=<v> <k>=<v> …]]<payload>
```

- **Sentinel**: literal `[[ct:` … `]]`. The `ct:` namespace — not the brackets —
  is what makes accidental reproduction unlikely; `[[ct:voice path=…]]` is not
  something a user types. The sentinel is a **single constant** in the builder,
  so switching to a stricter form (e.g. `⟦ct:…⟧`) later is a one-line change
  with no producer/consumer edits.
- **kind**: `voice` | `photo` | `file` | `telegram` | `mesh` | `pane` | `report`
  | `hook` (extensible — just a string).
- **attrs**: space-separated `key=value`. Values containing spaces/`]]` are
  percent-encoded (the builder encodes, the parser decodes). Keeps the line a
  single token and unambiguous.
- **payload**: the human/agent-readable remainder after `]]` — e.g. the voice
  transcript, or the user's actual message body for source prefixes. Optional.

Examples (producer → wire):
- voice (transcribed): `[[ct:voice file=/u/…/v.webm]]привет мир`
  (the transcript is the payload, so the agent reads it as the message)
- voice (no daemon): `[[ct:voice file=/u/…/v.webm note=no%20transcriber…]]` (UI shows the note)
- photo: `[[ct:photo file=/u/pic.jpg name=pic.jpg]]`
- telegram: `[[ct:telegram user=100000001 msg=12345]]do the task`
- mesh: `[[ct:mesh peer=peer-a pane=abc12345]]hi`
- from-pane: `[[ct:pane label=General%20Main pane=bda0e267]]ping`

The UI parser keys on the leading `[[ct:` so it never false-positives on the
payload; the agent sees a compact, readable tag followed by the real content.

## Architecture

### Rust: `src-tauri/src/message_markers.rs` (new)

```rust
pub const NS: &str = "ct";  // the single sentinel constant

pub struct Marker { kind: String, attrs: Vec<(String, String)>, payload: String }

impl Marker {
    pub fn new(kind: &str) -> Self;
    pub fn attr(self, k: &str, v: &str) -> Self;   // value percent-encoded
    pub fn payload(self, p: &str) -> Self;
    pub fn build(&self) -> String;                  // "[[ct:kind k=v]]payload"
}

pub struct ParsedMarker { pub kind: String, pub attrs: HashMap<String,String>, pub payload: String }
pub fn parse(s: &str) -> Option<ParsedMarker>;       // None if no leading [[ct:
```

Convenience constructors mirror the kinds: `voice(file, transcript|note)`,
`photo(file, name)`, `attachment(file)`, `telegram(user, msg)`, `mesh(peer,
pane)`, `pane(label, pane)`, `report(from)`, `hook(json)`.

### TS: `packages/chat-core/src/markers.ts` (new — shared by app + chat-ui)

Same surface mirrored: `buildMarker(kind, attrs, payload)`, `parseMarker(s)`,
plus typed helpers. chat-core is the right home — both the app (`chatPrefixes`)
and `chat-ui` (`MessageBubble`) already depend on it.

### Producers migrated (Rust)

- `voice_transcribe::format_attachment_line` → builds `ct:voice`/`ct:photo`/
  `ct:file` with `file`, optional `name`, and either the transcript as payload
  or a `note` attr.
- `bridge/manager` telegram/mesh prefix → `ct:telegram` / `ct:mesh`.
- `api/pane_routes` + `socket/server/pane` from-pane prefix → `ct:pane`.
- `service/hierarchy` report → `ct:report`.
- `bridge/manager` hook metadata → `ct:hook`.

### Consumers migrated (TS)

- `chatPrefixes.ts`: replace `PANE_RE` / `BRIDGE_RE` with `parseMarker()` →
  map `ct:telegram|mesh|pane|report` to the existing `ChatOrigin` shape.
- `MessageBubble.tsx`: `parseVoiceMessage` and the `\x00«file»`/`[attached]`
  attachment matcher → `parseMarker()` for `ct:voice|photo|file`.

## Error handling / edge cases

- `parse` returns `None`/`null` when the text doesn't start with `[[ct:` — plain
  text falls through unchanged (no behavior change for normal messages).
- Attr values are percent-encoded so payloads/labels with spaces, `]]`, quotes,
  or newlines never break the frame.
- Empty payload is valid (attachment-only markers).
- The agent-facing readability is preserved: kind + payload are plain text;
  attrs are compact `k=v`.

## Testing

Rust (`message_markers.rs` tests): build→parse round-trips for every kind;
percent-encoding of spaces/`]]`/quotes; `parse` returns None on plain text and
on a user-typed `[[ct:` without a valid frame; payload with embedded `]]`.

TS (`markers.test.ts`): mirror the Rust round-trip cases so the two
implementations stay in lock-step (the parity guard). Plus `chatPrefixes` maps
each source kind to the right `ChatOrigin`, and `MessageBubble` renders the
voice/photo/file card from the new markers.

Cross-language parity: a small fixture table of `(input, expected)` duplicated
in both test suites; CI for each language fails if its side drifts.

## Risks / mitigations

- **LLM confusion** — `[[ct:kind]]` is ordinary ASCII brackets + a namespace;
  trivially tokenized, no control bytes. Lower risk than rare unicode while
  keeping collision resistance via the `ct:` namespace.
- **Two implementations drift** — mitigated by the shared fixture table tested
  on both sides; the format is small and centralized.
- **Old transcripts** — render with the legacy shape (unparsed). Acceptable per
  policy; no migration of stored sessions.
- **Sentinel regret** — it's one constant; changing the bracket style later
  touches only the builder/parser, not the dozen call sites.
