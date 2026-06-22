# Snapshot test-bench — fast isolated parser/unit testing + agent-output library + drift runner

> Owner: Dante (orchestrator), from Andrey 2026-06-20. A SUPER-LIGHT, isolated test-bench:
> capture how agents respond in various situations → a snapshot LIBRARY → replay through
> our parsers IN ISOLATION (no live agent, no full CodeTerm, no UI) → assert. Milliseconds,
> dev-only, NO code duplication (thin wrapper over the REAL libs). Generalizes to ANY unit,
> not just parsing. Grows into a nightly DRIFT runner that catches agent-provider format
> changes early + adapts the plugins. Andrey: codex can build this decoupled from other
> work. Lives in CodeTerm, NOT shipped to prod (dev/CI only). Branch `plugins-rethink`.

## Why (the dev-speed win)
Today we verify parsers by spawning REAL agents on the dev daemon (evals/chat-roundtrip) —
slow (process spawn + boot + slow TUI render), flaky (timing), and serialized. Most parser
work needs none of that: it needs "given THIS screen/stream the agent produced, does our
parser extract the right thing?" That is a pure function over captured input → fast,
deterministic, parallel. The bench makes that the default; live capture becomes an
occasional step to REFRESH the library.

## Build on what exists (do NOT duplicate)
- **RT1 `.cast` capture** (`codeterm pane capture`) + `screen.txt` sidecar — the capture primitive.
- **`agent_state_replay.rs`** — already replays a `.cast` through SignalExtractor + classify
  (the isolated-runner pattern, Rust side).
- **`load_one` / parity_test** — runs a QuickJS provider plugin IN-PROCESS (no daemon).
- **per-plugin `plugin.test.ts` fixtures** (`model-picker.txt`, `wizard-multi-step.txt`,
  `question-picker.txt`) — ad-hoc snapshots; the library formalizes these.
- **evals/chat-roundtrip** — the LIVE harness; becomes the snapshotter's capture source.
The bench UNIFIES these into one fast, snapshot-driven, isolated runner.

## Architecture
1. **Snapshotter (capture → library).** A dev tool: drive a real agent into a labeled
   situation (reuse chat-roundtrip + RT1 capture), save a SNAPSHOT =
   `{ raw.cast, screen.txt (rendered, reviewable), meta.json (provider, version, situation,
   expected-parse) }`. Run occasionally (online) to BUILD/REFRESH the library; everything
   downstream is offline.
2. **Snapshot library.** `testbench/snapshots/<provider>/<version>/<situation>/…` — the
   corpus of "how agent X responds to Y" (model-picker, wizard, question, permission,
   normal-reply, rate-limit, …). Reviewable screen.txt + expected.json per case. Seed from
   the existing fixtures.
3. **Isolated runner (the fast core).** Feed a snapshot → run the TARGET parser IN-PROCESS,
   no daemon/UI/live-agent:
   - QuickJS plugin: `parsePrompt`, `classifyPaneState`, `hasEventMarkers`,
     `buildPromptResponse` via `load_one`.
   - Rust: `SignalExtractor`, `semantic_marker`, `command_detection`, `detector`.
   Assert the parse/extract vs `expected.json`; print a diff. Sub-second, parallel, no flake.
   Reuses the `agent_state_replay` replay + `load_one` — a thin wrapper, zero parser dup.
4. **Generalize — a test-bench for ANY unit.** The runner is just `input → call a real lib
   fn → assert`. Expose it generically (a small harness API + a case format) so any
   isolated unit (not only parsing) can get a fast snapshot test. The bench is the
   reusable stand.
5. **Maintainability.** Snapshots are human-reviewable (screen.txt); expected lives beside
   them; the runner auto-discovers cases; adding a case = drop a snapshot + expected. One
   command runs the whole library in seconds.
6. **(Future) nightly DRIFT runner.** A cron/CI job: capture FRESH snapshots from the
   latest installed agent-provider versions → run the parsers → DIFF vs the library →
   flag DRIFT (a new/changed TUI format the parser now misses, a renamed option, a new
   picker shape) → alert + open a fix task (semi-auto adapt the plugin) → so when a
   provider ships a new version that changes its output, we catch + adapt FAST, before it
   breaks in prod. This is the long-term payoff.

## Dev-only, no-dup, light
- Lives in CodeTerm (`testbench/` or `evals/testbench/`); uses the REAL libs as a
  dependency; NOT compiled into / shipped with prod. Fast, lightweight startup (no Tauri,
  no window, no daemon — just the lib + the snapshot).
- A thin wrapper ONLY — never re-implements a parser; calls the same code prod runs, so a
  green bench means the real parser is green.

## Tasks (codex, decoupled from the picker work)
- **TB1 — snapshotter**: a command/script that drives a real agent (reuse chat-roundtrip +
  `pane capture`) into a labeled situation and writes a snapshot `{cast, screen.txt, meta}`
  into the library. (Online; used to build the corpus.)
- **TB2 — library + seed**: the `testbench/snapshots/<provider>/<version>/<situation>/`
  layout + migrate the existing fixtures (model-picker / wizard / question / agent-state)
  into it with `expected.json`.
- **TB3 — isolated runner (fast core)**: replay a snapshot through the QuickJS plugin
  parsers (`load_one`) AND the Rust parsers (`SignalExtractor`/`semantic_marker`/
  `command_detection`/`detector`), assert vs expected, diff output. Reuse
  `agent_state_replay` + `load_one`. One command runs the whole library in seconds.
- **TB4 — generalize**: factor the runner into a small reusable bench harness/API + a case
  format usable for ANY isolated unit; document how to add a case.
- **TB5 (future) — drift runner**: nightly capture-latest + diff-vs-library + flag/alert +
  open fix task. (Plan now, build later.)

## Guardrails
- NO parser duplication — the bench calls the real lib functions; green bench == green prod.
- Dev/CI only — never linked into the prod binary.
- Snapshots reviewable (screen.txt) + expected beside them; adding a case is trivial.
- Offline-fast is the default; live capture (snapshotter) is the occasional refresh step.

<!-- revision: 1 -->
