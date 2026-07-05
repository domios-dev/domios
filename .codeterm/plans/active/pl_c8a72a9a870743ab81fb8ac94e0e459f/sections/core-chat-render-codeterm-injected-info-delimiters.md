---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_e17e2b4a70234455846d80dff550c48f
handle: core-chat-render-codeterm-injected-info-delimiters
title: Core Chat Render Codeterm Injected Info Delimiters
---

**HOW injected-info delimiters are made unambiguous.** The glue bug came from a one-sided marker: the prefix-builder emitted `-=-codeterm:…-=-` only at the *start* of an injected block, so the renderer had no boundary at the end and the next message glued on. The fix is a real envelope with a parser twin, implemented in `src-tauri/src/message_markers.rs` (Rust) mirrored by `packages/chat-core/src/markers.ts` (TS), kept honest by a parity fixture.

The envelope is namespaced and percent-encodes the structural characters so a payload can never reproduce the frame:

```rust
pub const OPEN: &str = "-=-codeterm:";
pub const CLOSE: &str = "-=-";

// Percent-encode the structural chars that would break the marker head; readable
// chars (incl. `/ . -` in file paths) pass through.
fn encode(value: &str) -> String { /* % space = \n \t " → %25 %20 %3D %0A %09 %22 */ }
```

`-=-codeterm:<kind> k=v k=v -=-<payload>` — the `codeterm:` sentinel plus percent-encoding makes accidental reproduction unlikely, so the renderer can confidently split the meta head from the real message: it parses the marker into a **distinct chip** (kind + attrs) and renders the payload as the message body. Two injected messages can no longer glue because each block's boundary is explicit and machine-detectable.

The canvas reuses this exact mechanism: provenance/source chips on cards (`-=-codeterm:pane …-=-`, `-=-codeterm:mesh …-=-`) parse through the same `markers.ts` twin, so meta never bleeds into rendered body anywhere.

## Key decisions
- **Rust + TS parity with a fixture** — the builder is Rust, the renderer is TS; `tests::PARITY` ↔ `markers.test.ts` proves both agree on the same inputs, so the two halves can't silently drift (the project's "native + JS parity in lockstep" rule).
- **Percent-encode only structural chars** — file paths stay readable (`/ . -` pass through); only `% space = \n \t "` are escaped, the minimal set that could break the head.
- **Sentinel over heuristics** — detection keys off the literal `-=-codeterm:` envelope, not "looks like metadata", so it's deterministic.
