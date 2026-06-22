# Provider Replay Quality Loop R15

Objective: improve and measure provider parsing quality across Claude, Codex,
Cursor, OpenCode, and synthetic fixtures without touching auto-approve or
auto-respond policy.

## Loop Shape

Run up to 15 rounds. Each round has three roles:

1. Research: inspect the corpus, parser surfaces, and live/recorded evidence.
2. Builder: add the smallest useful gate, fixture, replay, or focused fix.
3. Adversary: reject vacuous gates, provider-id leaks, copy-fragile parsing, and
   unproven claims.

The coordinator accepts a round only when the evidence is reproducible by a
command and the next gap is explicit.

## Quality Axes

- State correctness: `idle`, `working`, and `question` must match the visible
  terminal and structured prompt source.
- Transition stability: no safety-net idle fallback, no rapid
  working/question/idle strobes unless a replay explicitly documents an
  unavoidable transient.
- Question surfacing: PTY questions set state only; structured questions own the
  response route and pending prompt. Raw PTY options must not render inline cards.
- Display hygiene: `status_line` and `last_turn_summary` must not contain raw
  terminal chrome, ANSI/control bytes, long box-drawing dividers, or unrelated
  transcript content.
- Provider boundary: core code branches on capabilities and structure, not
  provider ids or vendor copy.
- Replay visibility: every fixed bug needs either a replay, a synthetic fixture,
  or a unit test explaining why a replay is not deterministic enough.

## Current Baseline

- Corpus count: Claude 14, Codex 12, Cursor 12, OpenCode 10, synthetic 2.
- `transition-latency --gate`: green, 0 safety-net fallbacks.
- `strobe-metric`: red when invoked correctly with `--glob '*/*/timeline.json'`;
  154 short segments in the current baked timelines.
- `permission-guard-check`: partial, one frame of pre-paint idle followed by
  120/121 question frames.
- `acceptance-suite`: red on old baked tail-working and mid-build question
  expectations; needs triage into true defects vs stale oracle policy.
- Existing corpus does not contain a Claude `status_line` with a long
  box-drawing divider tail, so the latest display bug was invisible to replay
  gates.

## Round Log

### R1: Status Display Hygiene

Accepted local evidence:

- Runtime fix: `offline_state_classifier::status` strips trailing terminal
  divider chrome before compacting a status line.
- Gate: `testbench/status_hygiene.py` scans rendered timelines for structural
  display leaks in `status_line` and `last_turn_summary`.
- Negative fixture:
  `testbench/fixtures/status-hygiene/divider-bleed.timeline.json` proves the
  gate fails on divider bleed, raw ANSI/control bytes, and path-only summaries.
- Current corpus result: 50/50 timelines pass the hygiene gate, confirming the
  latest Claude divider-tail bug was missing coverage rather than a known replay
  regression.

Commands run:

```bash
cargo test agent::offline_state_classifier::status --lib
python3 testbench/status_hygiene.py --self-test
python3 testbench/status_hygiene.py testbench/fixtures/status-hygiene/divider-bleed.timeline.json
python3 testbench/status_hygiene.py --corpus testbench/replays
./build.sh check
```

### R2: Acceptance RED Triage

In progress. Focus: explain why `acceptance-suite.sh` is red while
`transition-latency --gate` is green, and split the failures into current
defects, stale baked timelines, and oracle-policy problems.

Accepted visibility hardening:

- `testbench/strobe-metric.py --gate` now exits non-zero when strobes are
  detected.
- Empty matches and non-timeline JSON inputs now fail loudly instead of printing
  a false green.
- Diagnostic mode remains exit-code neutral so existing manual inspection
  workflows keep working.
- `testbench/rev19-acceptance.sh` now uses `strobe-metric --gate` in its
  offline regression section and preserves exit status instead of masking it
  through `tail` and `|| true`.
- `testbench/acceptance-suite.sh` now runs the status-hygiene gate as a first
  class display-text check.
- `testbench/permission-guard-check.sh` now evaluates only frames where the
  permission box is visibly painted. The synthetic replay has one empty
  pre-paint frame followed by 120 visible question frames; the gate now checks
  the meaningful 120/120 frames.

### R3: Agent-State Fixture Coverage

In progress. Current inventory from `tests/fixtures/agent-state`:

| Provider | Fixtures | Gating |
| --- | ---: | ---: |
| Claude | 3 | 0 |
| Codex | 4 | 2 |
| Cursor | 0 | 0 |
| OpenCode | 0 | 0 |

`cargo test --test agent_state_replay -- --nocapture` currently passes because
only two Codex seed fixtures are gating. Five real captures are measured as
baseline findings, not build failures. This is useful visibility but not enough
for "all providers" quality.

Next capture targets for this corpus:

- Cursor: `working_streaming`, `turn_complete`, `clarifying_question`, and a
  negative prose options case that must not become `question`.
- OpenCode: `working_streaming`, `turn_complete`, structured/PTY permission or
  question, and a negative prose options case that must not become `question`.
- Claude/Codex: promote or replace real baseline captures with curated seed
  fixtures only when the expected final state is stable enough to gate.

### R4: Temporal Coherence

Accepted local evidence. The remaining parser failures were not all frame-local parser mistakes:
terminal TUIs repaint non-atomically, so one frame can briefly miss the status
footer while the prior transcript still contains numbered prose or old picker
shapes. The fix direction is a temporal resolver, not a wider regex.

Target architecture:

1. Extract a raw per-frame candidate from structural signals (`working`,
   `question`, `idle`, `unknown`) with source confidence (`structured`,
   `status/footer`, `terminal-menu`, `offline-picker`, `composer`).
2. Resolve the candidate against recent state and clocks:
   - `Working -> Idle` may happen only after working grace or an explicit
     settled ready composer.
   - `Working -> Question` is immediate only for structured prompts. PTY/offline
     grid questions need either semantic settling or a picker anchored near the
     bottom/cursor interactive region.
   - `Idle -> Working` remains immediate on active status/footer.
   - `Question -> Working` remains immediate on active status/footer.
3. Keep structured prompts authoritative. PTY/offline questions set state only
   and must never fabricate inline cards.

Implemented for the current R15 failures:

- Move the `Working` grace hold before the ready-prompt fast path so a single
  mid-repaint prompt glyph cannot emit `Working -> Idle -> Working`.
- Gate offline picker questions by temporal/positional coherence:
  - If previous state is `Working` and semantic idle is still below grace,
    ignore offline picker detections.
  - Do not accept an offline picker found in old transcript text when the active
    bottom region is the agent composer. Prefer adding picker block coordinates
    to the detection result so this decision is structural and testable.
- Preserve true terminal questions (`terminal_questions_choice`) and structured
  prompts as immediate `Question`.

Required evidence:

- Re-render the two Claude snake replays and prove `mid_build_question == 0`.
- Re-render Claude/Cursor strobe samples and prove repaint-only
  `Working -> Idle -> Working` segments disappear.
- Re-render a real terminal question replay and prove it still ends in
  `Question`.
- Keep `transition-latency --gate`, status hygiene, and permission guard green.

Results after regenerating all 50 replay timelines from `raw.cast` using the
current `render_timeline` binary:

- `acceptance-gate.py`: GREEN.
- `transition-latency.py --gate`: GREEN, 0 safety-net fallbacks.
- `status_hygiene.py --self-test` and `--corpus testbench/replays --quiet`:
  GREEN, 50 timelines / 0 anomalies.
- `permission-guard-check.sh`: GREEN, 120/120 visible permission-box frames
  classified `question`.
- `strobe-metric.py --gate`: GREEN, 0 defect strobes. One raw short segment
  remains in `synthetic/repaint_proof_v1`, classified as supported short work
  because every frame has real working evidence (`status_line`/phase), so it is
  reported as `supported=1` rather than a defect.
- Claude snake replay `mid_build_question`: `2 -> 0`.
- Claude snake canonical `mid_build_question`: `34 -> 0`.
- Real terminal question replays still have `question > 0`, `card=0`,
  `native_picker=0`.

## Round Backlog

1. Add a display-hygiene gate for `status_line` and `last_turn_summary`.
2. Classify current strobe failures by provider and replay path; decide which
   are defects, stale captures, or expected transients.
3. Normalize gate invocation so matrix summary JSON files do not crash scripts
   that expect replay timeline objects.
4. Add or capture Claude completed-turn status chrome/divider replay evidence.
5. Add Claude spinner-after-completion replay coverage for the byte-idle vs
   semantic-idle flicker class.
6. Add Cursor status/path leakage coverage from the reported working row.
7. Add OpenCode structured permission/question E2E replay or live smoke coverage
   that distinguishes SSE pending prompt from PTY fallback.
8. Add Codex startup trust/update prompt coverage, including no inline card.
9. Add negative prose A/B/C coverage for all PTY providers.
10. Add terminal-open viewport/resize regression visibility if not already
    covered by Playwright.
11. Audit provider plugin parser parity after any new parser surface changes.
12. Audit restored panes and lazy-load launch text leakage against replay/eval
    coverage.
13. Re-run ROG/WebView2 Chat UI DOM order/state tests after frontend fixes.
14. Re-run full offline replay gates and live stop/structured-prompt gates.
15. Final adversarial pass: remove vacuous gates, document remaining capture
    gaps, and produce a commit-ready summary.

## Acceptance For A Round

- The report names exact replay paths or tests.
- New gates fail on a targeted bad fixture or explain why the current corpus has
  zero violations.
- Fixes are provider-agnostic unless located inside a provider plugin/module.
- `./build.sh check` and the relevant narrow test/gate are green.
- Auto-approve and auto-respond files are untouched.

<!-- revision: 2 -->
