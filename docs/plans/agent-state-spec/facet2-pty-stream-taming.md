# Facet 2: PTY Stream Taming → Structured Signals

The hard core. A PTY agent is a raw byte stream; every classifier downstream
(Facet 3 lifecycle, Facet 5 live-stream) is only as good as what it reads. Today
they read **one flat string** (`tail_text(60)`), and quiescence is **one bit**
(content-hash unchanged for 2s). That single bit conflates "pixels stopped" with
"turn finished" — the root of every false positive. This facet defines the
**SignalExtractor**: a provider-agnostic core layer that turns the byte stream
into a small structured `StreamSignals` value, the **input contract** for
`classifyPaneState` (Facet 3) and `classifyPaneStream` (Facet 5).

Boundary: core extracts **mechanical, structural** signals (grid math, byte/row
diffs, timers, regions, cursor). It does **not** classify and does **not** match
vendor copy. Semantic verdicts (busy fingerprint, "this is a menu", phase) stay
in the provider plugin. I produce the substrate; Facet 3/5 consume it.

## What exists, and what each gives

| Layer | Type | Gives | Misses |
|---|---|---|---|
| `OutputBuffer` (`pty/manager.rs`) | raw byte ring + 2 MB full history + monotonic `total_written` | exact bytes, `read_since(offset)` deltas, SSE/xterm replay | ANSI noise, no structure, no "what changed semantically" |
| `ScreenState` (`pty/screen_state.rs`) | per-PTY `vt100::Parser` 50×120×500 | de-ANSI'd **current grid** (`get_screen_text`/`tail_text`), formatted restore, `get_content_hash(exclude_bottom_rows)` | only the *latest* grid — no prev/delta, no timers, no cursor exposure, no region model |
| `PaneStatusTracker` (`agent/status.rs`) | Working/Waiting/Idle/Dead from screen-hash equality + `check_idle(2s)` | coarse parked edge for the reactor | **the false-positive engine** — hash≠ ⇒ Working (a spinner pins Working forever; a frozen spinner reads Idle/done) |

Key realization: **the vt100 parser is the ANSI/redraw equalizer.** A full-screen
TUI redraw (`\x1b[2J\x1b[H…`) and an incremental cursor-addressed repaint resolve
to the *same final grid*. So we never regex raw bytes for structure — we diff
**grid states**. `OutputBuffer` stays the transport/replay truth; `ScreenState`
stays "what the user sees now". This facet adds the missing third thing: **prev
vs current grid, over time** → deltas, regions, quiescence.

## The contract: `StreamSignals`

Produced by core, attached to the snapshot every classifier already receives.
`screenText`/`screenHash`/`rawChunkText`/`rawOffset` already named by Facet 5 are
*outputs of this extractor*. New fields below are the substrate Facet 3 needs to
infer `phase`/`lastMotionMs`/`AssistantPartial`.

```rust
pub struct StreamSignals {
    pub cols: u16,
    pub rows: u16,

    // --- the three views ---
    pub screen: GridView,        // current full grid (what the user sees)
    pub tail: String,            // last N non-blank rows (== today's tail_text)
    pub delta: ScreenDelta,      // what changed since the previous extraction

    // --- mechanical quiescence (the no-false-positive core) ---
    pub quiescence: Quiescence,

    // --- cheap structural pre-parse (provider-agnostic) ---
    pub cursor: CursorSignal,
    pub regions: RegionSignal,
}

pub struct GridView {
    pub lines: Vec<String>,      // trimmed-right per row, blanks preserved as ""
    pub last_nonblank_row: Option<u16>,
    pub screen_hash: [u8; 32],   // sha256 of trimmed non-empty rows (Facet 5's screenHash)
}

pub struct ScreenDelta {
    pub changed_rows: Vec<u16>,  // rows whose trimmed text differs from prev grid
    pub appended_text: Option<String>, // Some(_) iff change is a pure tail-append
    pub mode: DeltaMode,         // Append | InPlace | FullRepaint | None
    pub bytes_since_last: u64,   // total_written delta over the interval
    pub churn: Option<Region>,   // bbox {top,bottom} of changed rows
}

pub enum DeltaMode { None, Append, InPlace, FullRepaint }

pub struct Quiescence {
    pub byte_idle_ms: u64,       // since ANY byte (a live spinner emits → resets this)
    pub semantic_idle_ms: u64,   // since a change OUTSIDE the volatile region
    pub cursor_idle_ms: u64,     // since the cursor last moved
    pub volatile_only: bool,     // recent changes confined to spinner/status zone
    pub kind: Motion,            // mechanical verdict, NOT a lifecycle state
}

pub enum Motion { Active, Streaming, Animating, Still }

pub struct CursorSignal {
    pub row: u16,
    pub col: u16,
    pub visible: bool,           // DECTCEM (?25l/h) — vt100 already tracks it
    pub on_last_nonblank_row: bool,
}

pub struct RegionSignal {
    pub content: (u16, u16),     // [top,bottom) rows excl. banner + footer chrome
    pub volatile: (u16, u16),    // bottom K rows: status bar + spinner zone
    pub content_text: String,    // GridView.lines[content] joined — feeds partial streaming
}
```

### Two clocks, one rule (paused TUI vs finished turn)

The whole false-positive problem reduces to: **a change is not progress.**

- `byte_idle_ms` — reset by *any* byte. A live spinner emits bytes → low.
- `semantic_idle_ms` — reset *only* by a grid change **outside the volatile
  region**. A spinner repainting one footer row does **not** reset it.

The volatile region is the bottom `status_bar_rows` (already supplied per
provider via `agent_registry.status_bar_rows`, today used by `get_content_hash`'s
`exclude_bottom_rows`) **plus** any row that keeps changing with no net text
growth (an in-place spinner). Generalize the existing exclude-bottom-rows seed
into a first-class region model.

`Motion` is then mechanical and provider-free:

| byte_idle | semantic_idle | mode | → Motion |
|---|---|---|---|
| low | low | Append | `Streaming` (assistant text growing) |
| low | high | InPlace, volatile_only | `Animating` (spinner alive, no progress) |
| high | high | None | `Still` (nothing emitting) |
| low | low | FullRepaint→stable | `Active` then settles |

`Motion` is **not** a lifecycle verdict. The provider combines it with its own
structural busy-fingerprint (Claude's `looksBusy`: "esc to interrupt", "tokens)")
to reach `turn_complete` vs `working_streaming`. The decisive cases:

- **Finished turn:** `Still` + cursor parked on input row + provider sees no busy
  marker → provider may emit `turn_complete`. Mechanically distinguishable.
- **Frozen spinner** (blocked on slow Bash): `Still` (no bytes) **but** the busy
  marker text is still in the grid → provider veto to `working_streaming`. Core
  surfaces `Still`; the *grid still contains the marker*, so the provider's
  existing veto fires. No new core logic needed — this is why the veto stays
  provider-side.
- **Live spinner:** `Animating`, `volatile_only=true` → never read as done; the
  reactor's parked gate also never even fires (status stays Working, see below).

This kills the two known bugs at the source: a spinner no longer pins Working
forever (semantic_idle climbs), and a frozen spinner no longer reads idle-done
(the grid marker + provider veto).

## Feeding the status tracker from the extractor

Today `PaneStatusTracker.update_output` takes the raw screen-hash; hash≠ ⇒
Working. Replace its input with the extractor's **semantic-change** signal:
Working is reset only by `semantic_idle_ms == 0`. An `Animating` spinner no
longer keeps a pane Working; a genuinely streaming pane does. Same coarse enum,
correct edges — so the reactor's existing parked gate (`Idle|Waiting`) starts
firing at the *right* moments, which is the precondition for everything else.

## Incremental/delta computation (cheap, bounded)

Per pty keep a `PrevGrid { row_hashes: Vec<u64>, last_nonblank: Option<u16>,
cursor: (u16,u16) }`. On each `process` (already inside the `spawn_blocking` that
runs `ss.lock().process`):

1. vt100 reparse (already happening).
2. Hash each row (FxHash of trimmed text). O(rows) — 50 rows, trivial.
3. `changed_rows` = indices where `row_hashes[i]` differs.
4. **Append detection:** if every changed row index `> prev.last_nonblank` and
   all earlier rows unchanged → `mode = Append`, `appended_text` = the new rows.
   Else if changed rows ⊆ volatile region → `InPlace`. Else if `|changed| >
   0.6·rows` → `FullRepaint`. Else `InPlace`.
5. Update timers from an **injected clock** (`now_ms` parameter, mirroring
   `host.unixNowMs()` in `classify.ts`) so tests are deterministic — no sleeps.

This runs in the existing hot read path; cost is O(rows·cols) ≈ 6 K char-hashes
per chunk — within the hot-path discipline (no subprocess, no IO, bounded).

## Two consumers, two cadences, one extractor

The extractor holds per-pty state; two readers pull from it at different rates:

- **Classifier (Facet 3), gated:** the reactor reads `StreamSignals` only on the
  parked edge (Idle/Waiting), 1 Hz — unchanged cost profile.
- **Chat stream (Facet 5), continuous-throttled:** while a pane is `Streaming`,
  emit `regions.content_text` (diffed against last emit) at ~10 Hz to the chat
  UI. This is the **partial-message stream**: for line-append agents
  `appended_text` is the new text; for in-place TUI agents (Claude renders the
  reply inside a box) `content_text` is the re-rendered reply region, emitted as
  a growing message by stable id — exactly the merge-by-id model Facet 5
  describes. **No waiting for turn end.** Same delta+region machinery, two sinks.

## Examples

```
# Live spinner (Claude thinking) — 3 chunks, footer row only
chunk t=0   "⠋ Pondering… (3s · ↑ 1.2k tokens)"   footer row 49
chunk t=120 "⠙ Pondering… (3s · ↑ 1.3k tokens)"   footer row 49
chunk t=240 "⠹ Pondering… (4s · ↑ 1.4k tokens)"   footer row 49
→ changed_rows=[49] each tick, mode=InPlace, volatile_only=true
→ byte_idle≈0, semantic_idle climbs, Motion=Animating
→ status stays Working; classifier never gates; NOT done ✓

# Genuine finish
… assistant text fills rows 10..40, then stops; cursor → input row 45
chunk t=0   last assistant row painted
(no bytes for 800ms)
→ mode=None, byte_idle=800, semantic_idle=800, cursor on input row
→ Motion=Still; provider sees no busy marker → turn_complete ✓

# Frozen spinner (blocked on slow Bash, no output)
"⠹ Running… (12s · esc to interrupt)" then silence 5s
→ Motion=Still BUT grid still contains "esc to interrupt"
→ provider busy-veto → working_streaming (defer reap) ✓

# Resize repaint (false-finish trap)
SIGWINCH → full \x1b[2J\x1b[H… repaint of identical content
→ mode=FullRepaint, but row_hashes settle to PRIOR values
→ semantic_idle NOT reset (net content identical) → no spurious "new turn" ✓
```

## Fast-test approach

The extractor is **pure**: `(prev_state, chunks, now_ms) → StreamSignals`. No
daemon, no PTY, no sleeps.

- **Stream-script fixtures** (`.vtseq`): an ordered list of `{at_ms, bytes}`
  records. A harness feeds them through a fresh `vt100::Parser` + extractor and
  asserts the emitted `StreamSignals` sequence. Reuses Facet 6's `chunks.bin`
  capture format — these are the *same* raw captures, replayed with timing.
- **The false-positive wall** (each an assertion on `Motion`/`delta`, no
  classification):
  - spinner-only ticks → `Animating`, `volatile_only`, semantic_idle↑
  - frozen spinner → `Still` + grid retains marker (provider test owns the veto)
  - real finish → `Still` + cursor on input row
  - partial line typed then paused mid-line → cursor *not* on input row,
    `mode=Append` then `None` (distinguishes "typing paused" from "turn done")
  - full repaint / resize → `FullRepaint`, semantic_idle **not** reset
  - line-append shell output → `appended_text` exact (drives streaming)
- **Determinism:** inject `now_ms`; never call a real clock. (Mirrors how
  `classify.ts` already threads `host.unixNowMs()`.)
- **Native+JS parity:** the extractor is core-Rust only (no JS drop-in path —
  grid math isn't provider logic), so no parity case is owed; but the
  *serialized* `StreamSignals` shape is asserted against a JSON golden so Facet
  3/5 JS classifiers bind to a frozen contract.

## Risks

- **Fixed grid 50×120 vs real geometry.** The parser must be sized to the pane's
  actual cols/rows (a resize path exists in `PtySession::resize`). A wrong width
  miswraps rows → false `changed_rows`/`Append`. **Mitigation:** size the parser
  from pane geometry; on resize, reset `PrevGrid` (a repaint is expected, not a
  delta) and suppress one delta tick.
- **Spinner not in the footer.** Some TUIs animate inside the content region.
  The static bottom-K volatile region misses it. **Mitigation:** the dynamic
  rule (row changes repeatedly with stable stripped-content) catches in-content
  spinners; the static region is only the cheap fast path.
- **Append false-positive on scroll.** A scrollback shift looks like every row
  changed. **Mitigation:** vt100 handles scroll regions; diff the logical grid,
  not the viewport, and treat a uniform row-shift as `Append` (content slid up),
  not `FullRepaint`.
- **Throttled stream emit cost.** 10 Hz `content_text` diffs on many panes.
  **Mitigation:** emit only while `Motion=Streaming`; gate on
  `bytes_since_last > 0`; diff is O(content rows). Bounded, off the async runtime
  (runs in the existing blocking read task).
- **Clock injection leak.** If any production path forgets `now_ms` and reads a
  real clock, tests pass but prod drifts. **Mitigation:** the extractor takes
  `now_ms` as a required param — no default, no internal `Instant::now()`.

## Deliverable summary

A core `SignalExtractor` beside `ScreenState` that, per chunk, diffs the vt100
grid against the previous grid and produces `StreamSignals` — three views
(screen / tail / delta), a two-clock mechanical quiescence (`Motion`), cursor,
and a region model. It feeds (a) the status tracker's change signal (fixing the
spinner/frozen-spinner bugs at the source), (b) `classifyPaneState` as a richer
input than today's flat string, and (c) Facet 5's partial-message stream via the
same delta+region machinery. Pure, clock-injected, fixture-replayable.
