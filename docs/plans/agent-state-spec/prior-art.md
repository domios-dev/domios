# SignalExtractor — Prior Art & Reuse Audit

Audit of external prior art for `src-tauri/src/agent/signal_extractor.rs` (vt100 0.16
grid-diff → structured `StreamSignals`). Goal: don't reinvent wheels; identify what to
reuse. Research-only; no code changes proposed here beyond recommendations.

## REUSE vs BUILD — recommendation (read this first)

1. **vt100 diffing — BUILD (keep ours).** vt100 0.16 exposes only *escape-code* diffs
   (`contents_diff`, `rows_diff`, `state_diff`) meant for **re-rendering** one screen into
   another with minimal flicker. It has **no dirty-row / damage / changed-row-index API**.
   Our per-row FNV-1a hash diff that yields a `Vec<u16>` of changed rows is **not** a
   duplication of anything vt100 gives for free — it is the missing piece. Keep it.
   *Caveat:* we could lean on vt100 more in two spots — see "Lean on vt100 more" below.

2. **P1b fixtures — REUSE asciinema `.cast` (v2), with our own tiny reader.** The format
   is exactly our need (NDJSON: header line + `[time, "o", data]` event rows, timestamps
   in seconds, real-time-writeable). Adopt the **format and its ecosystem** (asciinema
   player/CLI for human inspection of fixtures) instead of a custom `chunks.bin`. But do
   **not** depend on the `asciinema` crate (CLI binary, not a library). Use a ~30-line
   serde reader, or the small `asciicast` crate if we want it off-the-shelf. Inject our own
   `now_ms` from the event `time` field for deterministic replay (we already inject time).

3. **State-detection lib — BUILD core, BORROW the idle-window idea (already have it).**
   No OSS library does provider-aware agent-lifecycle classification (turn_complete vs
   working_streaming vs rate-limit). The closest tools — **tui-use** and **microsoft/tui-test**
   — both use the *same* primitive we built: a **headless terminal emulator + debounce/idle
   window on render events** (tui-use defaults to 100ms silence; plus `--text <pattern>`
   semantic readiness). That validates our two-clock quiescence design. Nothing to adopt
   wholesale; our `byte_idle`/`semantic_idle` split is *more* capable than their single
   silence window. Optionally mirror their `--text` pattern matcher as a provider hint.

4. **xterm.js buffer for P5 streaming — VIABLE but keep Rust as source of truth.** The
   frontend already holds the rendered buffer; `term.buffer.active` + `@xterm/addon-serialize`
   can produce a structured grid / serialized snapshot client-side with zero re-render cost.
   But xterm.js has **no built-in dirty-line/diff API** either — we'd hand-roll the same row
   diff in TS, *duplicating* the Rust engine and forking the classification logic across two
   languages. Recommendation: **keep classification in Rust** (one brain), let the frontend
   xterm.js buffer be a *render/restore* convenience only. For P5 partial streaming, push
   Rust-computed deltas (`ScreenDelta.changed_rows` / `appended_text`) to the client; do not
   move state extraction into the browser.

5. **Net:** our architecture is sound and largely non-duplicative. The one concrete reuse
   win is **adopting the `.cast` format for P1b fixtures**. The one place to lean harder on a
   dependency is using vt100's `rows()` / `cell()` for attribute-aware regions if/when we need
   color/style signals (we currently only read text).

---

## 1. xterm.js grid/buffer & headless usage (P5 streaming)

- **Buffer API** (`@xterm/xterm`): `term.buffer.active` → `IBuffer` with `baseY`, `cursorY`,
  `length`; `getLine(y)` → `IBufferLine` with `translateToString()`; `getCell(x,y)` →
  `IBufferCell` exposing char + fg/bg color + bold/italic/etc. attributes. Structured grid
  access is fully available client-side.
  - Docs: https://github.com/xtermjs/xterm.js — typings: https://github.com/xtermjs/xterm.js/blob/master/typings/xterm.d.ts
- **`@xterm/addon-serialize`**: serializes the framebuffer to a string (ANSI) or HTML;
  designed exactly for "snapshot the terminal state and restore on reconnect."
  - https://www.npmjs.com/package/@xterm/addon-serialize
- **Headless / Node (no DOM)**: **`@xterm/headless`** runs the full parser + buffer in Node
  with no DOM; addons work as long as they avoid DOM APIs (serialize qualifies). So xterm.js
  *can* produce a structured grid server-side too.
  - https://www.npmjs.com/package/@xterm/headless
- **Change tracking**: **none built in.** Events are render-lifecycle (`onRender`,
  `onWriteParsed`, `onLineFeed`, `onCursorMove`) — *not* per-line damage. A diff between
  buffer states must be hand-rolled (same situation as vt100).
- **Tradeoff for P5**: Reading the existing frontend xterm.js buffer avoids a second VT parse
  and is "free" (buffer already populated by the visible pane). But it (a) only exists for the
  *focused/mounted* pane (xterm.js instances are per-visible-pane, not for every backgrounded
  agent), (b) has no diff primitive so we'd re-implement `changed_rows` in TS, and (c) splits
  the classification brain across Rust+TS. The Rust `SignalExtractor` runs for *every* pane
  regardless of UI mount and already holds time injection for deterministic tests.
  **Verdict:** Rust stays the source of truth; xterm.js buffer is a rendering/restore aid.

## 2. Headless VT screen-reconstruction libs

- **Rust `vt100` (ours, 0.16.2)** — confirmed via `src-tauri/Cargo.toml`.
  - Provides: grid model, `contents()`, `contents_between()`, `rows(start,width)`,
    `cell(row,col)`, `contents_formatted()`, `cursor_position()`, `hide_cursor()`, `size()`,
    `set_size()`, scrollback.
  - **Diff API = escape codes only**: `contents_diff(prev)`, `rows_diff(prev,start,width)`,
    `state_diff(prev)`, `input_mode_diff(prev)` — all return *terminal byte streams* to
    re-render `prev` into `self`. **No changed-row-index / dirty / damage reporting.**
  - **Did we duplicate vt100? No.** Our `Vec<u16> changed_rows` (row-hash diff) is the
    capability vt100 lacks. The `contents_formatted`/`state_diff` we *do* reuse (screen
    restore in `screen_state.rs::get_screen_formatted`). Good split.
  - Docs: https://docs.rs/vt100/latest/vt100/struct.Screen.html — crate:
    https://crates.io/crates/vt100
  - Note: a maintained fork exists — **`vt100-ctt`** (ChrisTitusTech) — same API surface, no
    extra diff capability; only relevant if upstream `vt100` stalls.
    https://github.com/ChrisTitusTech/vt100-rust , https://docs.rs/vt100-ctt
- **Python `pyte`** — in-memory screen with a real *dirty-line* set: `screen.dirty` is a set
  of changed line numbers, cleared each render. This is the closest analog to our
  `changed_rows` and confirms the design is standard prior art (just not present in vt100).
  https://github.com/selectel/pyte
- **`pexpect`** (Python) — process control + regex expect on *output stream* (not a grid);
  the canonical "wait for pattern" tool. Relevant to §4, not screen reconstruction.
  https://github.com/pexpect/pexpect
- **Go `vt10x`** (ActiveState/hinshun) — headless VT emulator with a cell grid and a global
  `Changed` flag; coarser than pyte's per-line set. https://github.com/hinshun/vt10x
- **Rust `expectrl`** — expect-style automation (spawn + match) over the byte stream; pairs a
  PTY with regex/EOF matchers. Good reference for a matcher layer, not for grids.
  https://github.com/zhiburt/expectrl
- **Takeaway**: pyte's `dirty` line-set is the one feature worth knowing we replicate; vt100
  simply doesn't ship it, so our hash diff is justified. Nothing to swap in.

## 3. PTY record/replay format for the fixture harness (P1b)

- **asciinema asciicast v2 (`.cast`)** — NDJSON. Line 1 = header JSON
  (`{"version":2,"width":W,"height":H,...}`); each subsequent line = `[time, code, data]`
  where `code` ∈ `"o"` (output), `"i"` (input), `"r"` (resize `"{COLS}x{ROWS}"`), `time` =
  float seconds since start. Designed for incremental real-time write and long-session replay.
  This is **almost exactly** our P1b need (timestamped raw chunks + resize, deterministic
  replay with injected timestamps — the `time` field *is* our injected clock).
  - Spec: https://docs.asciinema.org/manual/asciicast/v2/
- **Rust tooling**:
  - `asciinema` crate = the **CLI binary, not a library** (docs.rs explicitly: "not a
    library"). Don't depend on it. https://crates.io/crates/asciinema
  - `asciicast` crate — small serde-based library, `Header` + `Entry { time, event_type,
    event_data }`, `EventType` (Output/Input/...), v2 by default. Read+write. Maintained but
    low-activity (~0.2.x). Usable, or just inline a serde reader.
    https://crates.io/crates/asciicast , https://lib.rs/crates/asciicast
  - `asciicast-rs` (LegNeato) — older (0.2.2, dormant) variant of the same idea.
    https://github.com/LegNeato/asciicast-rs
- **Alternatives considered**:
  - **ttyrec** — binary `(timeval, len, payload)` frames; older, no resize event, sparse Rust
    tooling. No advantage over `.cast`.
  - **termtosvg** — a *renderer to SVG/animation*, not a capture/replay data format; wrong
    layer. https://github.com/nbedos/termtosvg
- **Verdict**: **Adopt `.cast` v2** as the P1b fixture format. Reuse the *format + asciinema
  player/CLI* for eyeballing fixtures; implement a tiny serde reader (or pull `asciicast`).
  We get human-inspectable fixtures and an existing ecosystem for free, vs a bespoke
  `chunks.bin` nobody else can open. Encode provider/state metadata in the header's custom
  fields. (v3 exists but v2 is simpler and universally supported by tooling.)

## 4. Detecting TUI/agent state from a terminal stream

- **tui-use** (onesuper) — agent-facing terminal driver. Core mechanism = **headless xterm
  emulator + debounce idle window**: "wait blocks until the screen has been stable for a
  configurable idle window (debounce)", default **100ms** silence; `wait --text <pattern>`
  gives a *semantic* readiness signal (prompt appeared) instead of pure silence. This is the
  same shape as our two-clock quiescence — but single-clock. Our `semantic_idle` (resets only
  on change *outside* the volatile region) is strictly more robust against spinner false
  positives. https://github.com/onesuper/tui-use
- **microsoft/tui-test** — E2E terminal test framework built on **xterm.js**; auto-waits for
  renders, `getByText(string|regex)` locators, `toBeVisible()` retry assertions. Same
  primitive (emulate + wait-for-render + text match). No lifecycle classification.
  https://github.com/microsoft/tui-test
- **`pexpect` / `expectrl`** — classic expect-pattern matchers on the stream; the "wait for
  known prompt" idiom we mirror in `screen_state.rs::screen_has_shell_prompt` /
  `screen_has_agent_tui`. (See §2/§3 links.)
- **Real-world failure modes worth noting for tests** — codex TUI issues where the spinner /
  busy-inhibitor stays stuck *after* task completion (process still busy, or stale spinner
  after subagent return). These are exactly the false-positive cases our `volatile_only` /
  `semantic_idle` split exists to avoid; good fixtures to add to P1b.
  - https://github.com/openai/codex/issues/19331
  - https://github.com/openai/codex/issues/16905
- **Verdict**: No library does provider-aware agent-state classification — that's our value.
  The field consensus (headless emulator + idle-window + optional text match) matches what we
  built and validates it. Optional small adoption: expose a per-provider `--text`-style
  ready/busy pattern hint alongside `VolatileHint`, mirroring tui-use's semantic signal.

<!-- revision: 1 -->
