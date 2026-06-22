# Agent-state detection — REAL-CASE testing plan

> Owner: Dante (orchestrator). From Andrey 2026-06-20: don't validate detection on
> synthetic fixtures only — drive REAL agents (claude/codex/opencode/cursor) into every
> state via real triggers, capture their actual PTY stream, and check whether we signal
> the state correctly. Addition to the agent-state phases (P1a built; P1b/P1c/P2 next).
> This is the empirical ground-truth + gate for the whole subsystem. Branch `plugins-rethink`.

## Why
Hand-written fixtures encode OUR assumptions about what a TUI looks like — they can't
surface the false-positives we don't know about. Real agents in real states do. Using
the live providers as fixture GENERATORS gives us: (a) a true golden+negative corpus,
(b) a measured BASELINE of current detection quality (concrete wait-FP / can't-tell-done
counts) BEFORE we touch P2, (c) a hard regression gate P2–P5 must beat.

## Two test layers (split by what each machine can run)
- **Layer A — CLI/PTY, runs on macOS NOW (no UI, no Playwright).** Drive agents headlessly
  via `codeterm agent spawn` + `codeterm send`, capture the real PTY byte stream to `.cast`,
  label ground-truth, replay through the SignalExtractor (P1a) + `classify_pane_state`,
  score. This is everything we can do while ROG is busy. Producible today.
- **Layer B — Playwright-UI E2E, runs on ROG/Windows (WebView2 CDP).** Same scenarios driven
  through the ACTUAL UI; assert the rendered status pill / question-option cards / done
  affordance match ground-truth. Top-layer real-world validation. ROG owns; we hand the
  scenario matrix + oracle over. Out of scope for macOS — do not attempt here.

## The capture mechanism (this IS P1b's capture half, built `.cast`-native)
A dev-only PTY TAP in the daemon: arm capture on a pane → tee its raw output bytes (ANSI
intact) + a per-chunk timestamp to an asciicast v2 `.cast` file (NDJSON: header +
`[t,"o",data]` rows; resize → `"r"` rows). The `t` float-seconds field IS the `now_ms`
the SignalExtractor needs for deterministic replay. Arm/disarm via a CLI subcommand
(`codeterm pane capture start|stop --pane ID --out FILE`) + REST. No bespoke `chunks.bin`.
Also dump a rendered `screen.txt` alongside for human review (reuse vt100 contents).

## The scenario matrix — (state × provider) with a CONCRETE trigger
Distinguish two kinds. **Content states** are easy to drive (just prompt). **Interactive
TUI states** must trigger the agent's OWN real machinery — do NOT ask the agent to *fake*
a menu in text; that's not the real fingerprint.

| Target state | Trigger | On-demand? |
|---|---|---|
| `working_streaming` | "write ~500 words on <topic>" (sustained output) | yes |
| `turn_complete` | any short task → wait for prompt to return | yes |
| `clarifying_question` | ambiguous task the provider asks back on; or a tool flow that prompts | mostly |
| `presented_options` / permission | run a tool needing approval WITHOUT auto-approve flag (real permission prompt / picker) | yes |
| `waiting_on_auth` | spawn while logged out / send `/login` | yes |
| `idle_stuck` | start a task that blocks (e.g. waits on stdin / network) then goes silent | mostly |
| `error_crash` | trigger a hard error (bad command / kill mid-tool) | yes |
| `rate_limited` | cannot force on demand → capture OPPORTUNISTICALLY + keep the recorded sample | no |
| `update_prompt` | appears when a CLI update banner shows → opportunistic + keep sample | no |

Cover claude + codex first (have classifiers today), then opencode + cursor (P4 zero-JS).
Seed reuses the existing `plugins/provider-codex/tests/fixtures/pane-state/*.txt` snapshots,
upgraded to timed `.cast`.

## Replay + oracle + scoring
- Replay each `.cast` through `SignalExtractor::step()` (timestamps from the cast) →
  `classify_pane_state` → a STATE TIMELINE.
- **Golden assertion:** at the final quiescent point the detected state == the labeled
  ground-truth state (+ extracted data: question text, options, reset-time).
- **Negative assertion (the hard gate):** NOWHERE mid-stream does it spuriously emit
  `turn_complete` (during a thinking pause) or `Waiting` (during a menu/animation). This
  is the wait-FP / can't-tell-finished class, expressed as a test.
- **Scoring:** per-state precision/recall + the count of false `Waiting`/`turn_complete`.
  Record the CURRENT classifier's score as the baseline; P2+ must strictly improve it.

## What we do NOW on macOS (concrete, while ROG works)
1. Build the `.cast` capture tap (P1b capture half) + the CLI `pane capture` command.
2. Build the replay+score eval set under `evals/agent-state/` (README + runner) — reuse
   the `evals/_lib` dev-locked `dc` wrapper; never hit prod.
3. Drive claude + codex through the on-demand rows of the matrix via `dc agent spawn` +
   `dc send` + `pane capture`; save labeled `.cast` + `screen.txt` into
   `tests/fixtures/agent-state/<provider>/<state>/`.
4. Replay through P1a + the CURRENT classifier → emit the baseline scorecard (this is where
   we SEE the real false-positives, with receipts).
5. Opportunistically grab `rate_limited` / `update_prompt` whenever they occur, bank them.

## Tasks (decomposable to workers)
- **RT1 (Rust/CLI):** `.cast` capture tap in the daemon (arm per-pane, tee raw+timestamps),
  `codeterm pane capture start|stop` + REST, `screen.txt` dump. Unit: capture→replay round-trips
  byte-identical; timestamps monotonic. (= P1b capture half.)
- **RT2 (Rust/TS):** replay+score eval under `evals/agent-state/`: `.cast` → SignalExtractor →
  classify → timeline; golden + negative oracles; per-state scorecard. Dev-locked.
- **RT3 (driver):** scenario-driver script — spawns real claude/codex, sends the per-state
  triggers, arms capture, saves labeled fixtures. Handles interactive-TUI triggers (real
  permission/login), marks opportunistic states as manual-capture.
- **RT4 (baseline):** run the matrix, commit the labeled corpus + the BASELINE scorecard to
  the spec folder. This is the artifact P2 is graded against.
- **RT5 (ROG/Windows, Layer B):** Playwright-UI E2E over the same matrix — assert rendered
  status pill / option cards / done affordance vs ground-truth. Hand the matrix + oracle to ROG.

## Sequencing & relation to the phases
RT1 (capture) → RT3 (drive+capture corpus) → RT2 (replay+score) → RT4 (baseline). This
folds INTO P1b (RT1 = its capture half; RT2 = its replay half) and produces the corpus that
gates P1c/P2/P3/P4. So starting this now is starting P1b — the right first build, and it's
fully macOS-runnable. P2's success is literally "the baseline scorecard's wait-FP count → 0".

<!-- revision: 1 -->
