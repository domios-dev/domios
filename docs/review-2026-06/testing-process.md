# CodeTerm Testing Process — Target Methodology

**Status:** PROPOSED — this document defines the process; backlog items D1–D8 in
[stabilization-backlog.md](stabilization-backlog.md) implement it. Once implemented,
the taxonomy section migrates into `docs/TESTING.md` (task D6) and this file becomes
the design record.

**Problem being solved.** Today tests are written ad hoc: no rule for which layer a
new test belongs to, no way to verify a new test actually tests anything, five
different runners with no aggregate result, fixed sleeps instead of polling, zero
retries on real-LLM tests, and four docs that disagree about layer names. Agents
writing tests have no process to follow, so every test is shaped by whichever existing
spec the agent happened to read.

---

## 1. Test taxonomy — one table, one truth

| Layer | Lives in | Runner | Real LLM? | Needs dev daemon? | CI | Budget/test |
|---|---|---|---|---|---|---|
| Rust unit | `src-tauri/src/**/tests.rs` | `cargo test --lib` | no | no | yes (D7) | <1s |
| TS unit | `src/**/*.test.ts`, `packages/**` | vitest (`pnpm test:unit`) | no | no | yes (D7) | <1s |
| Plugin | `plugins/*/tests/` | `pnpm test:plugins` | no | no | yes (exists) | <5s |
| API e2e | `tests/e2e/test_*.py` | pytest (`./build.sh e2e`) | no | yes (7686) | spike (D7) | <30s |
| UI smoke | `tests/playwright`, `@smoke` | `pnpm test:pw:smoke` | no | yes | local-only* | <10s |
| UI golden | `tests/playwright`, `@golden` | `pnpm test:pw:golden` | **yes** | yes | local-only* | <5min |
| WebDriver | `tests/webdriver/` | wdio | no | yes | local-only | <30s |
| Behaviour bench | `evals/<name>/` | `bash run.sh` via `_lib/lib.sh` | yes | yes (dev-locked) | never | unbounded |

\* Playwright stays out of hosted CI **by decision** (needs WebView2 desktop + claude
auth) — that decision is documented in `tests/playwright/README.md` and stands. The
compensating control is the pre-PR gate (§5), not hope.

### Which layer do I write? (decision tree)

```
Is it pure logic (parsing, formatting, state machine)?
├─ yes → unit test next to the code (Rust or TS). STOP — do not write a UI test
│        for something a unit test can prove.
└─ no → Does it require the daemon's HTTP/socket surface?
    ├─ yes, no UI → pytest API e2e
    └─ needs real rendering/interaction →
        ├─ deterministic, no LLM → Playwright @smoke
        ├─ involves a real agent/LLM → Playwright @golden
        └─ measures *behaviour quality* (prompt change, recall quality,
           routing) rather than pass/fail correctness → evals/ bench,
           NOT a test. Benches compare baselines; tests assert invariants.
```

Rule of thumb: **every bug fix gets a test at the LOWEST layer that can reproduce it.**
A cross-pane rendering bug whose root cause is a marker-parsing function gets a unit
test on the parser plus (at most) one golden test for the end-to-end path — not three
UI tests.

---

## 2. How to write a new Playwright test (the recipe)

Every new spec follows these steps **in order**. PR review checks them (§5).

**Step 1 — justify the layer.** One sentence in the PR description: why this can't be
a unit or API test (see tree above).

**Step 2 — use the fixtures, never raw setup.** All specs import from
`tests/playwright/helpers/fixtures.ts` (built by task D5 — the API below is a sketch
of the contract, D5's implementation defines the exact surface):

```ts
import { test, expect } from "../helpers/fixtures";

// fixtures provided:
//   ui   — per-worker CDP connection, health-rechecked between tests
//   api  — REST client against dev :7686, auto-reset after each test
//   pollUntil — see step 4

test("pane split renders two terminals @smoke", async ({ ui, api }) => {
  const ws = await api.createWorkspace("pw-split");
  await ui.splitPane(ws.activePane, "vertical");
  await expect(ui.panes()).toHaveCount(2);
});
```

Anti-pattern this replaces (real, from current suites): a module-level `beforeAll`
holding one shared connection — one crashed test poisons every test after it.

**Step 3 — tag it or it doesn't merge.** Exactly one tier tag in the title:
- `@smoke` — deterministic, <10s, no LLM, no network beyond dev daemon. Runs on every
  `test:all`.
- `@golden` — may spawn real agents / call real models. Bounded by an explicit
  `test.setTimeout`. Runs pre-PR and nightly.
- `@quarantine` — known-flaky, excluded from default runs, must have an entry in
  `QUARANTINE.md` (reason, owner, exit criteria, date). Quarantine without an exit
  plan is deletion with extra steps — prefer deletion.

Optional secondary tags: `@regression #<issue>` linking the bug it pins.

**Step 4 — no naked sleeps. Ever.** Fixed `setTimeout`/`waitForTimeout` is the #1
source of both flakes (too short under load) and wasted wall-clock (too long on fast
runs). Current suites do this (`cross-pane-chat.spec.ts:44` — fixed 2s poll loop);
new code uses the shared helper:

```ts
// helpers/poll.ts (task D5)
export async function pollUntil<T>(
  fn: () => Promise<T | null | false>,
  opts: { timeout: number; interval?: number; label: string },
): Promise<T> {
  const start = Date.now();
  let interval = opts.interval ?? 250;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - start > opts.timeout)
      throw new Error(`pollUntil timed out after ${opts.timeout}ms: ${opts.label}`);
    await new Promise((r) => setTimeout(r, interval));
    interval = Math.min(interval * 1.5, 2_000); // backoff, cap 2s
  }
}

// usage — self-documenting failure instead of a mystery timeout:
const msg = await pollUntil(
  () => api.lastChatMessage(paneId),
  { timeout: 120_000, label: `agent reply in pane ${paneId}` },
);
```

The ONLY acceptable fixed sleep is one with a comment naming the OS/SDK quirk it
covers (e.g. ConPTY resize debounce) — same standard as code comments.

**Step 5 — clean up what you create.** Workspaces/panes/agents created by the test are
removed in the test body or by the `api` fixture's auto-reset. A `@golden` test that
spawns agents must kill them in a `finally` — orphaned agents poison the next test's
pane count AND burn tokens.

**Step 6 — test data goes in `tests/fixtures/`.** Shared input data (JSONL transcripts,
config snapshots, marker samples) lives in `tests/fixtures/`, referenced by relative
path — never inlined as 200-line string literals in the spec, never duplicated per
suite. A generated fixture is committed only together with the script that regenerates
it.

**Step 7 — visual checks use the built-in screenshot API.** When a spec needs to
verify rendering beyond DOM assertions, use the daemon's own `/api/screenshot`
endpoint (dev :7686) rather than external capture scripts — it's what the endpoint
exists for, and screenshots land in a known location for the report. Screenshots are
debugging artifacts: assert on DOM/state, attach the screenshot.

**Step 8 — prove the test can fail (verification rule, §4).**

---

## 3. Tag contract enforcement (task D3)

`playwright.config.ts` gains projects so tiers are runnable and CI-addressable:

```ts
export default defineConfig({
  retries: 0,
  projects: [
    { name: "smoke",  grep: /@smoke/,  retries: 0 },
    { name: "golden", grep: /@golden/, retries: 1, timeout: 300_000 },
    { name: "full",   grepInvert: /@quarantine/ },
  ],
});
```

Plus a lint step (in `test:all`) that fails any spec whose `test(...)` title carries no
recognized tag. Untagged tests are unclassifiable and therefore unschedulable.

---

## 4. The verification rule: a test that has never failed is not a test

**Every new or materially changed test must be shown to fail when the behavior it
guards is broken.** This is the single highest-leverage rule in this document — it
kills assert-nothing tests, always-true assertions, and tests that pass because the
poll helper swallowed the result.

How to comply (pick one, state which in the PR):
1. **Bug-fix test:** run the test on the commit BEFORE the fix (`git stash` the fix or
   check out HEAD~1) — paste the red output into the PR, then the green output after.
2. **New-feature test:** sabotage the feature once (comment out the handler, return
   a wrong value), run the test, confirm red with a *useful* failure message, revert.
3. **Pure refactor of a test:** run it 3× consecutively green (flake check) and state
   that the assertions are unchanged.

Additionally, every NEW `@golden` test runs 2× consecutively green before merge —
LLM-path tests are where flake hides, and the second run costs less than the first
quarantine.

The PR template (task D9 creates `.github/PULL_REQUEST_TEMPLATE.md` — it does not
exist today) requires the chosen method + pasted evidence.
"Tests pass" with no failure evidence is not verification — it's a green checkbox of
unknown meaning. (This mirrors the project-wide evidence-before-assertion rule the
superpowers verification skill already imposes on agents.)

---

## 5. Running and gating

| When | Command | Must be green |
|---|---|---|
| during work | `cargo test <narrow filter>` / `pnpm test:unit -t <name>` | the touched area |
| before claiming done | `./build.sh test:all` (task D2) | all executed layers |
| before opening PR | `test:all` + `pnpm test:pw:golden` if agent paths touched | everything |
| CI on PR | Rust unit + TS unit + plugin tests (+ pytest spike) — task D7 | gate |
| nightly (local cron) | `pnpm test:pw --project=full` + golden | report, not gate |

`test:all` semantics: layers whose prerequisites are absent (dev daemon down, no
claude auth) **SKIP loudly** with a one-line reason — a silent skip reads as green and
that is how trust dies. Exit code reflects only executed layers.

**Flake policy.** A test that fails then passes on retry is logged, not ignored:
- `@smoke` retry budget is 0 — a flaky smoke test is a bug in the test or the product,
  fix or quarantine the same day.
- `@golden` retry budget is 1 (real LLM latency variance is real); a golden test that
  needs its retry >20% of runs goes to `@quarantine` with an exit plan.
- Quarantined >30 days with no progress → delete it. A permanently quarantined test is
  documentation pretending to be verification.

**LLM-test budgets.** Every `@golden` test declares `test.setTimeout(...)` explicitly
(no inherited defaults) and the value is the SLA: "if the agent can't report in 5min,
that IS the finding" — current `agents.spec.ts` comment, kept as policy.

---

## 6. What this replaces (gap analysis)

| Gap today (audit evidence) | Section that closes it |
|---|---|
| No rule for which layer a test belongs to; 4 docs disagree (TESTING.md vs playwright README vs TESTPLAN.md) | §1 taxonomy + tree (task D6) |
| No recipe for a new Playwright test; specs copy whatever spec the author saw | §2 recipe + fixtures (D5) |
| Tags advisory only; config has no projects (`README` defines tags, config ignores them) | §3 (D3) |
| No way to know a new test tests anything; no fail-first practice anywhere | §4 verification rule |
| Five runners, no aggregate; TS unit tests not in any script | §5 + `test:all` (D2) |
| `retries: 0` + fixed sleeps on real-LLM tests; no flake/quarantine policy | §4 helper + §5 policy (D4) |
| Rust/TS/plugin tests not in CI; provider tests don't re-run on daemon changes | §5 CI row (D7, D8) |
| Orphaned shell scripts hitting prod port 7685 from `tests/` | removed (D1) |
| Parity tests skip silently when plugins missing | loud-skip rule §5 (D8) |
| No PR template or review checklist carrying any of this into actual PRs | D9 |
| Evals produce output but never fail on regression (no committed baselines) | D10 |
| `.githooks/pre-push` exists but `core.hooksPath` never set — hooks run for nobody | A6 |
| Test data inlined/duplicated per spec; `tests/fixtures/` underused | §2 step 6 |

## 7. Risks & non-goals

- **Non-goal: Playwright in hosted CI.** Re-evaluate only if a self-hosted Windows
  runner ever appears. The pre-PR gate + nightly local cron is the compensating
  control; don't burn effort fighting the WebView2/auth wall.
- **WebDriver stays, with a narrow charter.** Playwright here is Windows-only (CDP
  attach to WebView2); WebDriver (wdio + tauri plugin) is the cross-platform UI
  fallback for macOS/Linux. Decision: keep both, but WebDriver covers ONLY what must
  be verified on non-Windows (platform-specific webview behavior, prompt-approval
  rendering) — it does not duplicate Playwright suites. If Playwright ever attaches
  cross-platform, fold WebDriver in and delete it.
- **Non-goal (for now): coverage metrics.** No tarpaulin/llvm-cov/vitest-coverage
  gates — coverage % is a vanity gate at this team size and would fight the
  real-daemon test style. Revisit if a regression ships that a coverage gate would
  provably have caught.
- **Risk: process theater.** Every rule above is enforced by a tool (config project,
  lint, `test:all`, PR checklist) — any rule that loses its enforcement gets deleted
  from this doc rather than kept as aspiration.
- **Risk: golden-test cost.** Golden tests call real models; keep the tier thin (one
  end-to-end path per feature, breadth lives in lower layers).
- **Migration is incremental.** Existing specs are NOT big-bang rewritten; D5 migrates
  2–3 reference suites, the rest convert when touched. New tests follow this doc from
  day one.

<!-- revision: 2 -->
