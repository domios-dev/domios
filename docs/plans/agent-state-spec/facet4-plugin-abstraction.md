# Facet 4 — Plugin abstraction + core↔plugin boundary

Scope: how a provider plugin *expresses* its state heuristics, the
`classifyPaneState` capability contract (input = Facet 2 `StreamSignals`, output =
Facet 3 taxonomy + confidence + evidence + action), the voting/confidence model
that prevents false positives, and the migration of the prompt-detection logic
still living in Rust core out into the plugins. Native+JS parity throughout.

Non-goals: the signal extractor (Facet 2), the enum taxonomy (Facet 3), the
fixture corpus (Facet 6), the chat-stream sink (Facet 5). I own the *seam*.

---

## 1. The boundary, today vs target

```
            TODAY                                    TARGET
  ┌─ core (Rust) ──────────────┐         ┌─ core (Rust) ──────────────┐
  │ detect_interactive_prompt  │         │ SignalExtractor (Facet 2)  │
  │ parse_interactive_picker   │  ──►    │ status change-trigger      │
  │ parse_permission_prompt    │         │ reactor: gate/threshold/   │
  │ has_event_markers default  │         │   auth-guard/execute       │
  │ detect_events default      │         │ asks ONE capability        │
  │ reactor (generic) ✓        │         └────────────┬───────────────┘
  └────────────┬───────────────┘                      │ StreamSignals
               │ flat screenText                       ▼
  ┌─ plugin (JS/native) ───────┐         ┌─ plugin (JS/native) ───────┐
  │ classifyPaneState ✓        │         │ classifyPaneState          │
  │ (re-detects prompts a 2nd  │         │  = manifest fingerprints   │
  │  time, divergent from core)│         │  + defineClassifier spec   │
  └────────────────────────────┘         │  + imperative extract/nav  │
                                          │ ONE detector, used by both │
                                          │ the gate AND the verdict   │
                                          └────────────────────────────┘
```

The load-bearing change: **prompt/menu detection exists once, in the plugin**, and
both the coarse status gate (Facet 2 feeds it) and the fine classifier read the
same matcher. Facet 1 gap #3 (two divergent detectors) dies here. Core never
matches a vendor string again.

**Invariant restated for this seam (AGENTS.md §Plugin discipline):** core asks
capabilities, never identities. After migration, `grep -nE '"(codex|claude|cursor|opencode)"' src-tauri/src/agent/{detector,state_reactor}.rs`
returns nothing, and `detector.rs`'s structural matchers are deleted, not
relocated to another core file.

---

## 2. Capability contract

One method, unchanged name, richer types. Rust trait (replaces `provider.rs:799`):

```rust
fn classify_pane_state(&self, signals: &StreamSignals) -> Option<PaneStateClassification>;
//                                    ▲ Facet 2, replaces today's PaneStateSnapshot{screenText}
```

`StreamSignals` (Facet 2) already carries `paneId/agentType/sessionId/cwd/status/
turnOwed/lastMotionMs` plus the grid views, `delta`, `quiescence.Motion`, cursor,
regions. The classifier reads structure, never re-derives it from raw bytes.

Output — Facet 3 tagged union + the three things core needs:

```ts
type PaneStateClassification = {
  state: AgentPaneState;     // Facet 3 discriminated union (kind + payload)
  confidence: number;        // 0..1 ensemble score for THE chosen state
  evidence: Evidence[];      // which signals fired — for tests + UI explain, not control
  action?: PaneStateAction;  // generic recommendation (keys | delayedResume | surface)
};
type Evidence = { id: string; weight: number; matched: boolean; rows?: number[] };
```

`PaneStateAction` stays exactly as it is (`pane_state.rs:84`): `keys`,
`delayedResume`, `surface`. The reactor already speaks only this vocabulary — no
change. `data` (resetAt/questions) folds into the Facet 3 state payload
(`rate_limited.resetAt`, `clarifying_question.question`) — one place, not two.

**Host-API audit (AGENTS.md discipline #4): this capability opens ZERO new
confinement holes.** Classification is pure over `(signals, host.unixNowMs())`.
The only host call is the existing clock (`classify.ts:91`), needed for
reset-time math and required-as-a-parameter so tests inject it. No FS, no exec,
no net. State this in the PR so the seam audit is a one-liner.

---

## 3. How a plugin authors heuristics — declarative first, imperative only where real

Three tiers, each used only when the one above can't express it. Most of a
provider's knowledge collapses into tier 1+2; tier 3 is the irreducible code.

### Tier 1 — manifest fingerprints (pure data, no JS)

A `paneState` block in `plugin.json`. Structural, declarative, parity-free
(identical native+JS by construction — both read the same JSON).

```jsonc
"paneState": {
  "statusBarRows": 3,                      // already exists under spawn; the volatile region (Facet 2)
  "busyMarkers": [                         // the looksBusy veto, declared not coded
    "\\besc to interrupt\\b",
    "\\bctrl\\+c to (cancel|stop|interrupt|quit)\\b",
    "\\bWorking[….]"
  ],
  "menu": { "optionRe": "^[ \\t]*([>❯▶➤])?[ \\t]*(\\d+)[.)][ \\t]+(.+?)[ \\t]*$",
            "selectedGroup": 1, "numberGroup": 2, "labelGroup": 3 },
  "promptReady": { "cursorOnInputRow": true, "noBusyMarker": true }  // turn_complete fingerprint
}
```

This alone gives a **baseline classifier for free**: busy-veto, menu parse,
working_streaming↔turn_complete. Opencode and cursor (no classify hook today,
Facet 1 §5) get real detection from manifest data without writing a line of JS —
the cheapest path off "fully blind".

### Tier 2 — `defineClassifier` spec (SDK helper, declarative matchers + tiny test fns)

The current plugins hand-roll the same loop: accumulate `confidence += weight`,
compare to threshold, pick a state, build evidence. Factor that loop into the SDK
(`@codeterm/plugin-sdk`) so a plugin *declares* matchers and the SDK runs the
vote, precedence, busy-veto, and evidence emission:

```ts
export const classifyPaneState = defineClassifier({
  threshold: 0.8,                          // == core GLOBAL_THRESHOLD; SDK asserts equality
  busyVeto: manifest.paneState.busyMarkers,
  precedence: ["waiting_on_auth","error_crash","rate_limited",
               "update_prompt","clarifying_question","presented_options",
               "turn_complete","idle_stuck"],          // Facet 3 ladder
  matchers: [
    { state: "rate_limited",
      signals: [
        { id: "wait+upgrade-menu", weight: 0.50, test: s => hasWaitAndUpgrade(s.menu) },
        { id: "limit-word",        weight: 0.25, test: s => /\blimit\b.../.test(s.tail) },
        { id: "reset-clock",       weight: 0.15, test: s => RESET_RE.test(s.tail) },
        { id: "confirm-footer",    weight: 0.10, test: s => CONFIRM_RE.test(s.tail) },
      ],
      onMatch: (s, ev) => ({                // imperative bits, tier 3, scoped to this state
        payload: parseReset(s.tail, host.unixNowMs()),
        action:  buildWaitAction(s.menu),   // navigation keys, never a blind Enter
      }) },
    // ...auth, clarifying, update matchers
  ],
});
```

The SDK's `runClassifier` does, in order: normalize CRLF, apply `busyVeto`
(→ `working_streaming`, conf 0), evaluate matchers in `precedence` order, sum the
matched weights per state, and return the **highest-precedence state whose summed
confidence ≥ threshold** with its `onMatch` payload/action and the full evidence
array. Below threshold for every actionable state → `turn_complete`/`unknown` per
the prompt-ready fingerprint, no action.

Why a spec, not free code: it makes the **voting model uniform and testable
across providers**, removes the copy-pasted accumulation loop (claude
`scoreRateLimit`/`scoreClarifyingQuestion` are the same shape), and yields
`evidence` for free (each signal's `{id,weight,matched}`), which is both the unit
test's assertion surface and the UI "why" (Facet 5).

### Tier 3 — imperative functions, scoped to one matcher

The genuinely-code parts: `parseReset` (clock+timezone → ISO), `buildWaitAction`
(compute `down/up/enter` from the highlighted index to the wait option —
`navigationKeys` in `classify.ts:154`), `hasWaitAndUpgrade`. These stay
hand-written but are *small, named, single-purpose, and individually unit-tested*.
They never touch global state or precedence — the SDK owns control flow.

**Rule of thumb:** a regex/glyph/row-shape → tier 1 manifest. A weighted vote →
tier 2 spec. A computation (parse a clock, count nav keystrokes, extract a
question string) → tier 3 function. If you're writing an `if (confidence > x)`
ladder by hand, you've leaked tier-2 logic into tier 3.

---

## 4. Voting / confidence model (the no-false-positive engine)

Five compounding guards, all expressible in the tier-1/2 declarations:

1. **Ensemble, not single signal.** A state's confidence = Σ matched weights;
   weights per state sum to 1.0. No lone signal reaches 0.8 — the actionable
   "anchor" signal (e.g. `wait+upgrade-menu` 0.50) needs ≥1 corroborator. This is
   already how `scoreRateLimit` works; the SDK just enforces the sum=1.0 invariant
   at load (reject a malformed spec) instead of trusting the author.
2. **Busy-veto precedes everything** (Facet 2 `Motion=Streaming/Animating` OR a
   manifest `busyMarker` in-grid). A menu rendered mid-turn never classifies.
   Today this veto is plugin-private and Layer 1 ignores it (Facet 1 §3) — after
   the migration the *same* veto gates the status edge, so the reactor stops
   firing on mid-stream menus at the source.
3. **Precedence ladder** (Facet 3 §Priority): auth and crash outrank everything,
   so a login screen with a menu beneath it is `waiting_on_auth`, never a
   key-press target.
4. **Auth-guard in core, not trusted to the plugin.** `state_reactor::decide`
   already converts any `waiting_on_auth` to surface-only even if the plugin
   attached keys (`state_reactor.rs:66`). Keep it — defense in depth; a buggy
   plugin still can't auto-answer an auth prompt.
5. **Threshold equality is asserted, not assumed.** SDK `threshold` must equal
   core `GLOBAL_THRESHOLD` (0.8); a parity test (§6) loads both and compares, so
   the plugin can never recommend a press the reactor would itself reject.

Confidence is a **vote score, never a probability** — say so in the SDK doc so no
one calibrates it. The only consumer is the `≥ threshold` gate.

---

## 5. Migration: Rust core → plugin

Move the four mislocated pieces (Facet 1 §2) out, delete the originals, and
collapse the two detectors into one.

| Core today | Destination | Mechanism |
|---|---|---|
| `detector.rs::detect_interactive_prompt` | plugin tier-1 `menu` + tier-2 matcher | the menu regex becomes manifest data; "is this a parked prompt" becomes a classifier verdict |
| `detector.rs::parse_interactive_picker` / `parse_permission_prompt` | plugin tier-3 `onMatch` extraction | returns `presented_options`/`clarifying_question` payload |
| `provider.rs::has_event_markers` default (literal `proceed`/`Esc to cancel`/`1.`+`2.`) | plugin `busyMarkers`/`menu` manifest, or DELETE | it's a raw-chunk prefilter; Facet 2's `delta.bytes_since_last>0` + parked gate replaces the "should I even look" question |
| `provider.rs::detect_events` default → `PermissionRequest` | plugin classifier (`presented_options` with `intent:"permission"`) | the event becomes a classified state, not a separate code path |

**Sequencing (each step ships green, parity-gated):**

1. Add `StreamSignals` to the snapshot (Facet 2) **without** removing anything —
   classifiers read new fields, old `screenText` still present. Pure addition.
2. Land `defineClassifier` in the SDK + port claude/codex's existing
   `score*` functions onto it. Behaviour-identical → the existing
   `classify-pane-state.test.ts` fixtures pass unchanged. This is the risky
   refactor; the fixture corpus (Facet 6) is the net.
3. Author the tier-1 `paneState` manifest block for all four providers; wire the
   **status gate** (Facet 2) to read the plugin's busy-veto + prompt detection
   instead of `detect_interactive_prompt`. Now one detector serves both.
4. Delete `detect_interactive_prompt`, `parse_interactive_picker`,
   `parse_permission_prompt`, and the `has_event_markers`/`detect_events`
   trait *defaults* from core. The trait methods either vanish or become
   `unimplemented`-style `None` (mirroring `classify_pane_state`'s default).
5. CI grep gate: no provider id and no menu-glyph literal in
   `src-tauri/src/agent/{detector,state_reactor}.rs`.

The native provider path (`agent/provider.rs` Rust impls) keeps a hand-written
classifier ONLY where a provider has no JS drop-in; for the bundled four, JS is
authoritative and native is the parity mirror (see §6). New providers are
JS-only — they never touch Rust.

---

## 6. Native + JS parity (AGENTS.md discipline #2)

`classify_pane_state` is exactly the kind of trait method a JS drop-in
implements, so it MUST have a parity case (the bar `parity_test.rs:268` already
sets). Today that case uses **one synthetic fixture** (Facet 1 §5) — insufficient.

Target: drive parity over the **real corpus**.

```rust
// for every fixture under plugins/<p>/tests/fixtures/pane-state/{positive,negative}/
for fx in load_corpus(provider) {
    let signals = build_signals(&fx);              // shared StreamSignals builder
    assert_eq!(js.classify_pane_state(&signals),   // QuickJS path
               native.classify_pane_state(&signals)); // Rust mirror (or both None)
}
```

- For the four bundled providers the Rust side is a thin mirror of the same spec
  (or, simplest, a fixture-table stub that returns the corpus's expected verdict
  — the parity test then proves JS reproduces the agreed golden, which is the
  real intent).
- A provider with **no native impl** must yield `None` on both paths
  (`parity_test.rs:296` already covers the default-None case) — keep it.
- The parity assertion compares the **full** `PaneStateClassification` (state +
  confidence + evidence + action), so a drop-in can't silently drift on the
  payload while matching the state.

QuickJS caveat (AGENTS.md discipline #4): the classifier uses only `String`,
`RegExp`, `Date`, and `host.unixNowMs()` — all present in QuickJS. No `console`
dependence (the silent-abort bug class). Lint the spec for any other host call.

---

## 7. Fast-test approach

Pure functions, no daemon, no PTY, no sleeps — the whole seam is
`(signals, now_ms) → classification`.

- **Spec unit tests** (per matcher, per signal): assert each `signal.test`
  fires/doesn't on a one-line input; assert weights sum to 1.0 at load.
- **Voting tests:** one matched signal → below threshold → no action; anchor +
  corroborator → ≥ threshold → exact action. Busy-veto input → `working_streaming`
  regardless of a visible menu. (Mirrors `scoreRateLimit` cases, now SDK-level.)
- **Precedence tests:** auth+menu → `waiting_on_auth`; busy+question →
  `working_streaming`; rate+upgrade → `rate_limited` (Facet 3 precedence rows).
- **Golden corpus** (Facet 6 owns the files): positive → `(kind, conf≥0.8, exact
  action, payload)`; negative → no action. The CI gate fails the build if any
  negative fixture yields an action (Facet 1 gap #11).
- **Parity** over the real corpus (§6) — native==JS on every fixture.
- **Determinism:** inject `now_ms`; the SDK forbids an internal clock read, same
  as Facet 2's extractor.

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| `defineClassifier` becomes a leaky framework (too clever) | keep it to the 4 primitives: busyVeto, precedence, weighted signals, onMatch. No conditionals, no cross-matcher state. If a provider needs more, it's tier-3 code, not a new spec feature. |
| Refactoring `score*` onto the spec changes behaviour | step 2 is gated by the *unchanged* existing fixtures; any diff is a real regression, caught before deletion of the old core path. |
| Manifest regex authored wrong (catastrophic match) | tier-1 strings are structural and unit-tested like any signal; SDK compiles them at load and rejects an invalid/over-broad pattern (e.g. matches empty string). |
| Deleting core detectors breaks the status gate | the gate is rewired to the plugin detector *before* deletion (step 3 precedes step 4); parity proves the plugin detector agrees with the old core one on the corpus first. |
| A drop-in provider ships no classifier | manifest tier-1 gives a safe baseline (busy-veto + menu + turn_complete); absent even that, core default `None` → reactor no-ops (safe, blind, honest). |
| Sum-to-1.0 invariant fights real authoring | weights are per-state and normalized by the SDK at load if the author opts in (`normalize:true`); the assert is the guardrail, not a straitjacket. |

---

## 9. Deliverable summary

A single capability — `classify_pane_state(StreamSignals) → PaneStateClassification`
— authored in three tiers (manifest fingerprints → `defineClassifier` spec →
scoped imperative functions), voting with an enforced ensemble + precedence +
busy-veto + core auth-guard + asserted-equal threshold, with the prompt-detection
logic migrated out of `detector.rs`/`provider.rs` so core never matches a vendor
string and one detector serves both the status gate and the verdict. Zero new
host holes. Native+JS parity over the real corpus is the anti-drift gate.

<!-- revision: 1 -->
