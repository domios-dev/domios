# Facet 1 — Current state-detection architecture + gap audit

Scope: map what classifies a PTY agent's state today, *where each piece runs*
(Rust core vs provider plugin), pinpoint the wait-false-positive and
can't-tell-if-finished roots, inventory AS1–AS7, and rank the gaps.

---

## 1. Two detection layers (the load-bearing fact)

State detection is split across **two independent layers** that do NOT share
code, and the boundary between them is the source of most problems.

```
PTY bytes
   │
   ▼
┌─ LAYER 1 — coarse status (RUST CORE, ad-hoc) ───────────────────────────┐
│  PaneStatus = Working | Waiting | Idle | Dead   (agent/status.rs)        │
│  • Working : screen-tail content HASH changed  (status.rs::update_output)│
│  • Waiting : detect_events() => PermissionRequest                        │
│              => detector.rs::detect_interactive_prompt (RUST fingerprint)│
│  • Idle    : hash unchanged for idle_timeout_s (background.rs::check_idle)│
│  • Dead    : PTY exit                                                    │
│  Decided in service/output.rs::process_pty_output on every chunk.        │
│  THIS LAYER GATES the reactor and IS what the UI/CLI sees.               │
└──────────────────────────────────────────────────────────────────────────┘
   │ only on the parked edge (Idle|Waiting)
   ▼
┌─ LAYER 2 — fine classification (PLUGIN, clean) ─────────────────────────┐
│  PaneState = Working | WaitingOnAuth | UpdatePrompt | RateLimited        │
│            | ClarifyingQuestion | IdleStuck | Unknown  (pane_state.rs)   │
│  provider.classify_pane_state(snapshot) -> PaneStateClassification       │
│  • ensemble of STRUCTURAL matchers, weighted, threshold 0.8              │
│  • busy-veto (looksBusy), auth-first, navigation keys, reset-clock parse │
│  Implemented: claude (classify.ts), codex (plugin.ts). NOT opencode/cursor│
└──────────────────────────────────────────────────────────────────────────┘
   │
   ▼
state_reactor.rs::decide  → keys inject | T7 delayed-resume | surface event
```

Layer 2 (AS1–AS4) is the *intended* design and is clean: provider-owned,
ensemble-gated, generic verdicts. Layer 1 is the *legacy* status machine that
predates it — and it is the one that actually decides "is this pane parked",
which is the gate Layer 2 depends on. **The two layers re-detect the same thing
(an interactive prompt) with two different, divergent code paths.**

---

## 2. Where each piece runs (Rust core vs plugin)

| Concern | Location today | Should live |
|---|---|---|
| Working/Waiting/Idle/Dead status | `agent/status.rs` + `service/output.rs` (CORE) | core *change-detector* only; truth from plugin |
| "is screen an interactive prompt?" | `agent/detector.rs::detect_interactive_prompt` (CORE) | **PLUGIN** — provider-structural |
| parse prompt question/options | `detector.rs::parse_interactive_picker` (CORE) | **PLUGIN** |
| `has_event_markers` prefilter (`❯`,`proceed`,`Esc to cancel`,`1.`+`2.`) | `provider.rs` trait default (CORE) | **PLUGIN** (literal-ish, leaks vendor copy into core) |
| `detect_events` => PermissionRequest | `provider.rs` trait default (CORE) | **PLUGIN** |
| busy / streaming detection (`looksBusy`) | claude/codex `classify.ts` (PLUGIN) ✓ | plugin ✓ — but unused by Layer 1 |
| fine PaneState classification | claude/codex plugins (PLUGIN) ✓ | plugin ✓ |
| reset-clock / timezone parse | claude `classify.ts` (PLUGIN) ✓ | plugin ✓ |
| confidence threshold / auth-guard / gate | `state_reactor.rs` (CORE) ✓ | core ✓ (generic) |
| status string to UI | `hierarchy.rs` `format!("{:?}")` (CORE) | core, but typed not stringly |

**Mislocated (move OUT of Rust):** `detect_interactive_prompt`,
`parse_permission_prompt` / `parse_interactive_picker`, the `has_event_markers`
default, and the `detect_events => PermissionRequest` default. These are
provider-shaped (menu glyphs, footer keywords, selector chars) sitting in core
as the trait *default*, so every provider that doesn't override inherits a
hardcoded structural fingerprint living in `agent/detector.rs`. That is exactly
the "logic stuck in Rust core" the brief calls out.

---

## 3. Root cause — wait false-positives ("looks waiting but isn't")

The misread happens at **Layer 1**, before the clean classifier ever runs.

1. **No busy-veto on the gate.** `Waiting` is set whenever
   `detect_interactive_prompt` matches a numbered-menu-near-footer shape
   (`detector.rs:57`). A menu rendered *mid-stream* — a tool-call list, a
   plan the model is printing, an echoed option set — trips it. The plugin's
   `looksBusy` veto (`classify.ts:171`: `esc to interrupt`, `Working…`,
   `ctrl+c to cancel`) exists precisely to reject this, **but it lives in
   Layer 2 and Layer 1 doesn't consult it.** So the pane is marked `Waiting`
   regardless; only later does the classifier (if the provider has one) veto.
2. **Duplicate, divergent detectors.** Layer 1 uses
   `detect_interactive_prompt` (Rust); Layer 2 uses `parseClaudeMenuOptions`
   (JS). Same job, two code paths, no parity test between them. They disagree,
   and Layer 1 wins the gate.
3. **Hash-as-oracle is fragile.** `Working` = "content hash changed"
   (`status.rs:51`). An animated footer (spinner, elapsed timer) changes the
   hash every tick → pane is *permanently* `Working` even while parked at a
   prompt. Inversely, a fully static genuine prompt produces no hash change →
   times out to `Idle`, never `Waiting`. `status_bar_rows` exclusion mitigates
   only the rows a provider declares.

Net: `Waiting` is a structural-fingerprint guess with no busy gate, and
`Working` is a hash heuristic that an animation can pin or a static prompt can
starve.

---

## 4. Root cause — can't tell if finished

**There is no completion signal anywhere.** `Idle` is defined as *silence*:
hash unchanged for `idle_timeout_s` (`background.rs:412` → `status.rs::check_idle`).

- A slow/thinking agent that pauses output ≥ timeout is **indistinguishable**
  from a finished one. Both read `Idle`.
- The structure every agent actually emits at turn-end — the input box
  redrawn, a `>` ready prompt, a token/cost footer, and the *absence* of the
  `esc to interrupt` spinner — is **never parsed into state**.
- Ironically the inverse signal already exists and is reliable: `looksBusy`
  in the plugin. "Not busy + prompt-box present" ≈ "turn finished, input
  ready". It is used only as a private veto inside `classify`, never surfaced
  as the authoritative Working↔Done discriminator.

**Missing signal (the single highest-value fix):** a provider-supplied
liveness/turn-boundary verdict, e.g. `{ busy: bool, promptReady: bool }`,
sourced from the same structural knowledge already in `looksBusy`. This is also
the exact signal needed to drive real-time streaming of partial assistant
messages (flush while `busy`, finalize on the `busy→promptReady` edge) — so it
unlocks both correct `Idle` *and* streaming.

---

## 5. AS1–AS7 inventory (what exists)

| Track | Status | Evidence |
|---|---|---|
| **AS1** capability + generic types + reactor | DONE | `pane_state.rs`, `state_reactor.rs` (gate, threshold 0.8, auth-guard, per-episode dedup, T7/keys/surface). Started in `lib.rs:863`. |
| **AS2** codex UpdatePrompt + Skip-not-Update | DONE | `provider-codex/src/plugin.ts::scoreUpdatePrompt` (weights .45/.20/.15/.10/.10), fixtures `update-prompt-skip-option2/3.txt` + negatives. |
| **AS3** claude RateLimited + reset parse + T7 resume | DONE | `classify.ts::scoreRateLimit` + `parseResetClock`, fixtures `rate-limit-*.txt`. |
| **AS4** ClarifyingQuestion + WaitingOnAuth | DONE | `classify.ts::scoreClarifyingQuestion` / `scoreAuthPrompt`, fixtures `clarifying-question.txt`, `auth-login.txt`. |
| **AS5** state-model surfacing to UI + idle-stuck | **NOT DONE** | `IdleStuck` enum exists but **no classifier ever returns it**. `pane-state-surface` event has **zero frontend consumers** (grep of `src/` = empty). Rich PaneState never reaches UI. |
| **AS6** no-FP corpus + voting harness + parity | PARTIAL | per-plugin vitest (`classify-pane-state.test.ts`) + native/JS parity (`parity_test.rs::classify_pane_state_native_js_parity`). **No CI gate** that the negative corpus stays action-free; parity uses one synthetic fixture, not the real corpus. |
| **AS7** CLI-not-installed (Case C) | **NOT DONE** | no launch-error classifier / install-recipe seam found. |

Fixture corpus today: claude 9 files, codex 6 files (mix positive + negative).
Opencode/cursor: **no `classify_pane_state`** → trait default returns `None`
(`provider.rs:803`) → reactor no-ops for them (safe but fully blind).

---

## 6. Reactor gating (AS1) recap

- **Loop:** own thread, 1s poll (`POLL_INTERVAL`), 60-row tail snapshot.
- **Gate:** only `Idle|Waiting` panes (`decide()` early-returns otherwise) —
  inherits Layer-1's verdict, including its false `Waiting`.
- **Dedup:** `reacted: HashSet` — fires once per parked episode, cleared when
  the pane leaves parked. Prevents re-press every tick.
- **decide():** parked ∧ `confidence ≥ 0.8` ∧ `state != WaitingOnAuth` ∧
  `action.is_some()` → `Act`; auth → `Surface`; else `Ignore`.
- **Execute:** `action_to_writes` → `inject_raw_keys_async` (send --actions
  path) | `delayed_send::create_message` (T7) | `surface_to_user`
  (notification + `pane-state-surface` emit).
- **Safety inheritance bug-class:** the reactor is only as honest as Layer 1.
  A false `Waiting` on a mid-stream menu *does* reach the classifier; today the
  plugin's busy-veto saves claude/codex, but a provider without a busy-veto (or
  without a classifier) gets no protection at this seam.

---

## 7. How state flows to the UI today

- **Layer 1 only.** `GET /api/panes/{id}/status` → `hierarchy.rs:185`
  `format!("{:?}", get_status(pty_id))` → `PaneStatusInfo.status: String`.
  **Stringly-typed**, coarse (Working/Waiting/Idle/Dead), Debug-formatted.
- **Layer 2 reaches nothing renderable.** Classification → reactor →
  `create_notification` + `emit_everywhere("pane-state-surface", …)`. No
  frontend listener exists. The fine state (which question, which reset time,
  auth vs update vs rate-limit) is computed and **dropped** at the UI boundary.
- Consequence: the UI cannot show "asked a question / rate-limited until 4:20 /
  waiting on auth" — only the four coarse strings.

---

## 8. Ranked gap list

**Ad-hoc (heuristic where structure exists):**
1. `Working` = hash-changed and `Idle` = silence-timeout. No completion signal;
   animation pins Working, static prompt starves to Idle. (§3, §4) — **root of
   can't-tell-finished.**
2. `Waiting` gate has no busy-veto; mid-stream menus → false Waiting. (§3) —
   **root of wait-false-positive.**
3. Two divergent prompt detectors (Rust `detect_interactive_prompt` vs JS
   `parseClaudeMenuOptions`) with no parity between them.

**Mislocated (in Rust core, belongs in plugin):**
4. `detect_interactive_prompt` + `parse_interactive_picker` (structural menu
   detection) in `agent/detector.rs`.
5. `has_event_markers` + `detect_events` trait defaults in `provider.rs` carry
   literal vendor-ish strings (`proceed`, `Esc to cancel`, `Tab to amend`).
6. The Working/Waiting/Idle *truth* is core-owned; should be a plugin liveness
   capability with core only running the cheap hash as a change-trigger.

**Missing:**
7. No `busy`/`promptReady`/turn-boundary capability — blocks honest Idle AND
   streaming of partial messages.
8. `IdleStuck` ("turn owed but parked") is never produced — no detector wires
   it (needs "a turn is owed" context core has but the snapshot omits).
9. No frontend consumer of `pane-state-surface`; fine PaneState never rendered
   (AS5).
10. Opencode + cursor have no classifier — blind for two live providers.
11. No CI negative-corpus gate (AS6); regressions in false-positive rate are
    not caught.
12. AS7 (CLI-not-installed) absent.
13. `PaneStateSnapshot` is screen-text only — no parsed transcript tail, no
    "turn owed" flag, no prior-state — limiting what any classifier can decide.

---

## 9. Fast-test approach (for the redesign this facet feeds)

- **Golden fixtures already exist** (`tests/fixtures/pane-state/<provider>/`):
  positive asserts `(state, conf≥0.8, exact action)`; negative asserts
  no-action. Keep + grow; promote every observed false-positive into the
  negative corpus.
- **Layer-1↔Layer-2 parity**: once prompt-detection moves into the plugin, one
  detector serves both the gate and the classifier — kills gap #3 and makes a
  single fixture authoritative for both.
- **Liveness fixtures**: capture busy frame, finished frame, slow-pause frame;
  assert `busy`/`promptReady` so completion detection is unit-tested without a
  live PTY (mirrors `status.rs` tests but on real screen tails).
- **CI gate**: fail the build if any negative-corpus fixture yields an action
  (AS6), run native+JS over the *real* corpus, not one synthetic fixture.

<!-- revision: 1 -->
