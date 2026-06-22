# Playwright Test Hardening — Implementation Plan

> **Sub-orchestration under Dante.** This is a subordinate track of the
> plugins-rethink effort. Dante (orchestrator on peer `eight`, workspace
> `codeterm-alpha`) owns the master integration line. This track runs on its own
> branch `plugins-rethink-tests` (off `plugins-rethink`) on Mac-Air. When this
> branch is green and the auditor signs off, **Dante merges it** into the master
> line. Coordinate the merge step with Dante's plan before integration.

**Goal:** turn the Playwright suite from a burden into a genuinely runnable and
*reviewable* thing — replayable traces you can watch in the player, a fixtures
layer that kills copy-paste, no hidden sleeps/hacks, lint guardrails that keep it
clean, a repeatable "test reviewer" that judges per-test fitness, and the
highest-value UI coverage gaps filled.

**Workers:** Codex on gpt-5.5. Each track is a self-contained task: read this
whole plan + your track, implement ONLY your track, strict TDD where applicable
(red→green→commit, nothing left red), run the repo gates (`pnpm test:pw:lint`,
the touched suite), then `codeterm agent report DONE/BLOCKED`. Workers never spawn
workers — report up. The orchestrator re-runs the gate itself before declaring a
track done; never on the worker's word.

---

## Diagnosis (verified by read-only recon, 2026-06-18)

16 specs, ~61 tests, 3452 LOC, single-worker against the live dev daemon over
WebView2 CDP (port 9223) + REST (7686).

- **🔴 No replay at all.** `playwright.config.ts` has `trace`/`video`/`screenshot`
  all OFF, reporter is `list` only, retries 0. `show-trace`/`show-report` have
  nothing to open → "watch the replay to judge a test" is impossible today.
- **🟠 Fixtures layer exists but is abandoned.** `helpers/fixtures.ts` exports
  `api`+`ui` fixtures; only 2/16 specs use them. The other 14 hand-roll
  `new ApiClient()` + `connectUi()` at module scope with their own try/finally.
- **🟠 Copy-paste without extraction:** spawn-agent ×5, DOM-marker poll ×3,
  pane-cleanup ×3, media/upload-mock ×4.
- **🟠 Hardcoded sleeps / hacks:** 9 — `waitForTimeout(400)`,
  `setTimeout(r, 10_000)` "give the CLI a moment", `setTimeout(20_000)` "survive a
  health loop", `force:true` clicks masking timing.
- **🟢 Healthy:** selectors mostly stable (~110 `data-testid` vs ~9 brittle
  `.first()`), no zombie tests, QUARANTINE empty, `.catch(()=>{})` only in cleanup.
- **Coverage gaps (UI):** Settings, MemoryPanel UI, Extensions/plugin-manager UI,
  ChatView rendering have no Playwright coverage.

---

## Tracks

### Track I — Replay infrastructure  *(foundation, lands first, solo)*
Enable post-mortem + the player.
- Modify `tests/playwright/playwright.config.ts`: `trace: 'retain-on-failure'`
  (+ `on-first-retry`), `video: 'retain-on-failure'`, `screenshot: 'only-on-failure'`,
  add the HTML reporter alongside `list`, set `outputDir`, set a sane `retries`
  policy per project.
- Add `package.json` scripts: `test:pw:report` (`playwright show-report`),
  `test:pw:trace` (open a trace), and a `test:pw:debug` convenience.
- Update `tests/playwright/README.md` with the replay workflow.
**Acceptance:** a deliberately-failing test produces a `trace.zip` + HTML report;
`npx playwright show-trace` and `show-report` open them. No green-suite behaviour
change. **Blocks A and the run targets.**

### Track A — Test reviewer ("ревизор")  *(parallel after I)*
A repeatable auditor that judges test fitness and emits replayable artifacts.
- **Static analyzer** (`tests/playwright/helpers/audit-static.mjs`): scan every
  spec for anti-patterns — sleeps (`waitForTimeout`/bare `setTimeout`), tests with
  no `expect`, `force:true`, `networkidle`, brittle selectors, inline setup vs
  fixture. Emit machine-readable JSON.
- **Trace analyzer** (`tests/playwright/helpers/audit-trace.mjs`): parse each
  produced `trace.trace` and score per-test fitness — action count, real waits vs
  sleeps, assertion presence, screenshots — and link each test to its trace for
  human replay in the player.
- Output `tests/playwright/AUDIT.md` (per-test verdict + trace links) + a
  `test:pw:audit` script that runs the suite with tracing then the analyzers.
**Acceptance:** `pnpm test:pw:audit` produces `AUDIT.md` with a per-test fitness
verdict and a trace path the human opens in the player. Depends on Track I.

### Track F — Fixtures adoption + dedup  *(parallel)*
- Migrate the 14 non-fixture specs onto the `api`/`ui` fixtures.
- Extend `helpers/fixtures.ts` with the missing shared helpers: `spawnAgent()`,
  `pollMarkerInDom()`, auto `paneCleanup` (fixture teardown), a media/upload mock
  helper. Delete the ×5/×3/×4 duplicated blocks.
**Acceptance:** 16/16 specs use fixtures; the duplicated blocks are gone; suite
still green (orchestrator-verified). Disjoint from S/L by file ownership where
possible; coordinate the shared `fixtures.ts` edits with Track S.

### Track S — Sleep / hack eradication  *(parallel)*
- Replace all 9 hardcoded sleeps/`waitForTimeout`/`setTimeout` loops with
  `pollUntil`/web-first assertions; remove `force:true` by fixing the underlying
  wait; remove any `networkidle`.
**Acceptance:** zero `waitForTimeout` and zero bare `setTimeout`-poll in specs;
suite green.

### Track L — Lint guardrails  *(parallel, small)*
- Extend `helpers/check-tags.mjs` (or add eslint-plugin-playwright) to ban:
  `waitForTimeout`, `networkidle`, `force:true`, a `test()` with no `expect`, and
  raw `new ApiClient()`/`connectUi()` outside fixtures.
**Acceptance:** `pnpm test:pw:lint` fails on a planted violation of each rule and
passes on the cleaned suite. Makes the F/S cleanup stick.

### Track C — Coverage gaps  *(after F/S/L; bigger)*
New suites, each on fixtures from day one, for the highest-value holes:
`13-settings/` (theme apply, config edit, keybinding display),
`14-memory-ui/` (search, preview, relate — paired with the 06-mem API suite),
`15-extensions/` (plugin-manager UI: list, detail, configure-with-AI) — **plus
the two surfaces Dante added on the master line, explicitly in scope here:**
(a) **plugin-activity progress UI** — inline in the Extensions panel + a global
toast, driven by the `plugin-activity` event over SSE/Tauri; (b) **uninstall
confirm dialog** with an "also remove downloaded data" checkbox (his L3
clean-uninstall, `--remove-data`).
ChatView rendering (message blocks, code, image attach).
**Acceptance:** each new suite green, tagged, fixture-based, lint-clean; (a) and
(b) covered.

### Track T — Test affordances (app-side stabilization)  *(after integration; enables C)*
Make the app test-friendly at the source instead of having tests work around it.
We control both code and tests, so additive/prod-inert affordances are fair game.
- **T1 — stable selectors:** add `data-testid`/aria to components the tests target
  through brittle/text/`.first()` selectors (driver: the auditor's findings).
  Additive attributes, zero prod behaviour change. Enables Track C (its new
  Settings/MemoryPanel/Extensions/ChatView suites need stable anchors).
- **T2 — test-env mock seam:** extend the existing `VITE_CODETERM_TEST` /
  `CODETERM_TEST_CLOCK` flags into a coherent test mode that deterministically
  mocks the non-deterministic — a fake LLM/agent reply path, a
  getUserMedia/MediaRecorder stub, deterministic time — so UI tests stop waiting
  on real multi-minute LLM turns where the test is not about the LLM. **Entirely
  behind the test flag; no prod effect.**
- **T3 — test handles:** minimal test-only seams (e.g. seed a pane into a target
  state) to kill setup boilerplate and flaky waits.
- **T4 — flip `PW_LINT_STRICT` on by default** once selectors are stable, so the
  Track L guardrail is permanently active (it is opt-in today).
**Scope coordination:** Track T touches `src/` (and possibly `src-tauri/`) =
Dante's domain. Coordinate the exact component/file scope with Dante BEFORE
dispatch so additive affordances do not collide with his fast-moving master.
**Acceptance:** affordances additive + prod-inert (prod build unchanged in
behaviour); the mock seam makes a previously LLM-dependent UI flow deterministic
under the test flag; `PW_LINT_STRICT` green by default. Runtime confirmation on
ROG (Track V).

### Track V — Verification  *(orchestrator-owned, final)*
- Full suite with tracing on **macOS local** (API layer) and a **Windows
  cross-platform pass on ROG** over WebView2 CDP (real UI replays).
- Produce HTML report + traces; orchestrator reviews artifacts + the `AUDIT.md`.
**Run-target note:** desktop UI-driving tests require Windows (WebView2 CDP), so
the UI replays come from ROG via mesh. **ROG is time-sliced (one Windows box).**
Handshake with Dante (confirmed): I own the ROG Playwright UI-replay pass and fold
in surfaces C(a)+C(b); Dante has a small non-UI ROG check pending (transcriber
Windows zip-bootstrap). He pings pane `8ec85e2c@eight` before grabbing ROG; I ping
him when my run starts — never collide.
**Rebase discipline:** `plugins-rethink` moves fast (Dante's tracks merge
continuously) but `tests/playwright` is disjoint from his src/src-tauri/cli/plugins
changes — rebase `plugins-rethink-tests` from it periodically to stay clean.

---

## Orchestration

- Sequencing: **I → (F ‖ A ‖ L) → S → [integrate] → T → C → V.** T (app-side
  affordances) lands before C because C's new suites depend on T's stable
  selectors + mock seam. F and S both rewrite the same spec
  files, so they are **serialized, not parallel** (running both in parallel
  worktrees would collide on every spec — the orchestration lesson). F lands the
  fixture-based specs first; **S then eradicates sleeps on those fixture-based
  specs.** A (auditor: new helper files + `package.json` audit script) and L (lint:
  `check-tags.mjs`) touch disjoint files, so they run **in parallel with F**.
  A needs I (traces) — already done.
- Worktrees: each parallel worker runs in its own git worktree off
  `plugins-rethink-tests` (`pwt/fixtures`, `pwt/auditor`, `pwt/lint`), so concurrent
  workers never share a checkout. **Merge order: A, F, then S, then L last** — L's
  bans must run against the F/S-cleaned specs, else the gate goes red on
  un-cleaned specs. Branch→content map tracked; nothing left stranded.
- **Gate ownership:** the orchestrator (me) re-runs `pnpm test:pw:lint` + the
  touched suite in the warm checkout before accepting any track. Red branches
  bounce back with the failure.
- **Pre-dispatch review:** an Opus reviewer pass over each worker's diff before it
  folds in, per the orchestration playbook.
- **Merge handoff:** when `plugins-rethink-tests` is green end-to-end and `AUDIT.md`
  is clean, **Dante merges it** into the plugins-rethink master line. This track
  does not self-merge into `plugins-rethink`.

<!-- revision: 1 -->
