# Agent State Detection — Master Spec (synthesis of facets 1–6)

> Owner: Dante (orchestrator), synthesizing 6 design agents (Andrey 2026-06-20).
> Goal: a generalized, elegant agent-state-detection system that lives in the
> PROVIDER PLUGINS, tames the raw PTY stream into reliable signals (kill wait
> false-positives, know when a turn is DONE), and unlocks real-time partial
> streaming + chat-UI/state-machine improvements. Facet docs alongside this file
> hold the detail; this is the unified design + phased plan. Branch `plugins-rethink`.

## The two root causes (facet1) this whole design exists to fix
1. **Wait false-positive:** the coarse Rust "Waiting" gate (Layer 1: detector.rs +
   status.rs) has NO busy-veto — the plugin's `looksBusy` lives in Layer 2, DOWNSTREAM
   of the gate — so a mid-stream menu/animation reads as Waiting.
2. **Can't-tell-finished:** there is NO completion signal — "Idle" is a mere
   silence-timeout, indistinguishable from a thinking pause. Turn-boundary structure
   (prompt-box ready, absence of "esc to interrupt") is never parsed into state.
Plus: TWO divergent prompt-detectors (Rust core vs plugin); Layer-2 rich state has
ZERO UI consumers; opencode/cursor have no classifier; logic mislocated in Rust core.

## Core ↔ plugin contract: REGISTER + CONSUME (data-over-code)
Core is a GENERALIZED engine + a registry; plugins REGISTER declarations into it and
CONSUME signals/helpers from it — core never knows a provider.
- **Plugin REGISTERS (declarative data):** volatile-region hints (spinner/status the
  semantic clock ignores), busy/promptReady + menu/prompt fingerprints (manifest
  `paneState`), session-store descriptor (A4.2). Core stores + applies them generically.
- **Plugin CONSUMES:** `StreamSignals` (input to classify), shared SDK helpers
  (`defineClassifier` voting, `host.jsonl`/`host.chat`, `host.toolcall.parse`), host APIs.
- Imperative plugin code only for REAL computation (reset-time parse, menu nav,
  question extraction). The more expressed as registered DATA, the cleaner — that's how
  opencode/cursor get a classifier with ZERO JS. Core never matches a vendor string.

## Architecture — one pipeline, plugin-owned heuristics
```
PTY bytes
  -> [CORE] SignalExtractor (facet2): vt100 grid diff -> StreamSignals
       { screen (rendered tail), transcriptTail, delta (what-just-changed),
         quiescence (TWO clocks: byte_idle vs semantic_idle), cursor, regions }
       + now_ms injected (deterministic replay)
  -> [PLUGIN] classify_pane_state(StreamSignals) -> PaneStateClassification (facet3/4)
       { state: AgentPaneState, confidence, evidence[], action? }
  -> [CORE] PaneStatusTracker fed from the SEMANTIC-change signal (not raw hash);
       reactor gates on the parked edge; executes generic action (keys / delayed
       resume / surface) — never matches a vendor string
  -> [UI] PaneAttentionState (facet5): one contract -> status pill, question/options
       cards, auto-resume, done/idle-stuck affordances, live partial stream
```

### Substrate — StreamSignals (facet2), the input contract
Core `SignalExtractor` diffs the vt100 grid vs its previous state. Key: the
**two-clock quiescence** — `byte_idle` (reset by ANY byte) vs `semantic_idle` (reset
only by changes OUTSIDE the volatile spinner/status region). Semantic-idle is what
mechanically separates a paused/animating TUI from a finished turn. Same delta+region
machinery feeds the live partial stream. MUST: size the vt100 parser to real pane
geometry + reset PrevGrid on resize; `now_ms` is a required param (no internal clock).

> **BOUNDARY (critical — answers "is the extractor in the plugin?"):** the
> SignalExtractor is in CORE but is STRICTLY PROVIDER-AGNOSTIC — pure, generic
> terminal mechanics (vt100 diff → screen/tail/delta/quiescence/cursor), identical for
> every PTY, knows NO provider and matches NO vendor string. It lives in core (not a
> plugin) only because it's heavy, stateful, byte-level emulation over the raw stream
> that already lives in core, and reimplementing it per-plugin in QuickJS would be
> wasteful + wrong. The ONE place provider-specificity could leak is the **volatile
> `regions`** (which screen area is spinner/status "noise" the semantic clock must
> ignore) — that is **PLUGIN-DECLARED** (a manifest hint) and fed INTO the generic
> extractor, NOT hardcoded in core. ALL semantic interpretation (busy/promptReady,
> menu/option parsing, state fingerprints, reset-time) lives in the PLUGIN's
> `classify_pane_state`. RULE OF THUMB: if you ever want to hardcode a provider-
> specific thing in the SignalExtractor, that is the signal it belongs in the plugin
> (as a declared region/pattern, or in classify) — core must stay generic.

### Output — AgentPaneState taxonomy (facet3)
`working_streaming{phase,partial}` · `turn_complete{promptReady}` ·
`clarifying_question{question,options}` · `presented_options{options,intent}` ·
`waiting_on_auth{method}` · `rate_limited{resetAt,tz}` · `update_prompt` ·
`error_crash{category}` · `idle_stuck{reason}` · `unknown`. + `PromptOption`,
`OptionIntent`, `AssistantPartial`. Maps cleanly from today's `PaneState`. A
per-state × per-provider FINGERPRINT CATALOG (structural shapes, never vendor strings).

### Boundary — how plugins author heuristics (facet4), 3 tiers
1. **Declarative manifest `paneState`** — busyMarkers/menu/promptReady fingerprints as
   pure data (regex/shape). Parity-free. Gives opencode/cursor a baseline classifier
   with ZERO JS.
2. **SDK `defineClassifier`** — factors the copy-pasted confidence loop into ONE voting
   engine (weighted ensemble + precedence ladder + busy-veto + evidence emission).
   Load-time invariants: weights sum to 1.0 per state; `threshold == GLOBAL_THRESHOLD`.
3. **Imperative `onMatch`** — small scoped fns only for real computation (reset-clock
   parse, nav-key math, question extraction).
The 4 prompt-detectors mislocated in Rust core (detect_interactive_prompt,
parse_interactive_picker, parse_permission_prompt, has_event_markers/detect_events)
MIGRATE into the plugins → ONE detector serves both the status gate and the fine
verdict (kills the divergent-detector class + the FP). Zero new host-API holes
(classification is pure over signals+clock). Native+JS parity over the real corpus.

### Liveness — the single highest-leverage signal (facet1 #1)
A provider `{busy, promptReady}` liveness signal (sourced from the existing `looksBusy`
knowledge + facet2 semantic-quiescence): feeds the Waiting busy-veto AT the gate,
gives an honest completion signal (`turn_complete = promptReady && !busy && quiescent`),
AND gates partial streaming. One signal fixes both root bugs + unlocks streaming.

### Streaming + UI (facet5)
Provider exposes the partial (in `working_streaming.partial` / a `classify_pane_stream`).
VT-rendered screen snapshot is the CANONICAL source for live partials → replace-by-id
(redraws/backtracking replace, never append duplicates). One `PaneAttentionState`
contract drives: live status pill, interactive question/options cards, typing indicator,
auto-resume, done/idle-stuck affordances. Wire Layer-2 to the UI (today: zero consumers).

### Testing (facet6) — the no-false-positive gate
Capture raw PTY per provider per state as `chunks.bin` (ANSI reality) + generated
`screen.txt` (reviewable); deterministic replay (timed stream-script, `now_ms` injected).
Golden corpus (state → classification + extracted data + confidence≥threshold) AND a
NEGATIVE corpus (partial-redraw, prose, mid-stream menu → NO false wait/done/action) as
the HARD release gate. Run BOTH the TS plugin corpus tests and the Rust QuickJS/ScreenState
corpus tests so plugin heuristics and core deserialization/reactor decisions can't drift.

## Phased plan
- **P1 — substrate + linchpin (unblocks all):** build the SignalExtractor (vt100 diff,
  two-clock quiescence, now_ms-injected) + the fixture capture/replay harness (facet6) +
  the SDK `defineClassifier` voting engine; port existing claude/codex `score*` fns onto
  it BEHAVIOUR-IDENTICALLY, gated by the unchanged fixture corpus.
- **P2 — fix the bugs at the source:** feed PaneStatusTracker from the semantic-change
  signal; add the `{busy,promptReady}` liveness + busy-veto AT the gate; add the
  `turn_complete` completion signal. → kills wait-FP + can't-tell-finished.
- **P3 — consolidate detectors:** migrate the 4 Rust prompt-detectors into the plugins;
  rewire the status gate to the plugin detector (parity-proven over the real corpus)
  BEFORE deleting detector.rs; CI grep gate: no provider-id / menu-glyph literal in
  detector.rs / state_reactor.rs.
- **P4 — taxonomy + coverage:** adopt the AgentPaneState enum; add opencode/cursor
  classifiers (manifest-fingerprint baseline, zero-JS); negative-corpus CI gate (the
  hard release gate from facet6).
- **P5 — UI + streaming:** PaneAttentionState contract; wire Layer-2 to the UI (status
  pills, question/options cards, auto-resume); `classify_pane_stream` + VT-snapshot
  partial streaming to chat UI.

## Top risks / guardrails
- vt100 geometry: size to real pane + reset on resize (else false append/repaint deltas).
- Determinism: `now_ms` injected everywhere (fixture replay must be timing-independent).
- No-false-positive: the NEGATIVE corpus is a HARD gate, not aspirational; weights-sum-1.0
  + threshold==GLOBAL_THRESHOLD enforced at load.
- Migration safety: parity over the real corpus BEFORE deleting any Rust detector.
- Boundary: core stays generic (asks the capability, executes generic actions); zero new
  host-API holes; native+JS parity for the full classification.

> Detail per facet: facet1-current-and-gaps · facet2-pty-stream-taming ·
> facet3-taxonomy-and-fingerprints · facet4-plugin-abstraction ·
> facet5-streaming-and-ui · facet6-testing-and-fixtures.

<!-- revision: 1 -->
