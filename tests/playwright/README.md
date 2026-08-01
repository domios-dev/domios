# Playwright — isolated dev console

Drives a **real, dedicated** CodeTerm dev console (Tauri, named instance
`playwright`, port 7786 by default), not a browser mock. The interactive dev
instance on port 7686 and its `~/.codeterm-dev` store are never adopted,
restarted, or mutated by this suite.

For the canonical layer taxonomy and the "which layer do I write?" decision tree,
see [`docs/TESTING.md`](../../docs/TESTING.md).

## Tooling decision (spike result, 2026-06-06)

- On Windows the Tauri window is **WebView2**, which speaks the Chrome
  DevTools Protocol when the dev Tauri config launches it with
  `additionalBrowserArgs` containing the named instance's CDP port.
  Playwright attaches via `chromium.connectOverCDP` — full control of the
  actual app UI, real Tauri IPC, no shimming.
- Pure browser-side Playwright against the Vite server is **not viable**:
  the frontend calls `@tauri-apps/api` `invoke()` directly with no HTTP
  fallback, so outside a Tauri WebView the UI is non-functional.
- The old Tauri WebDriver/WDIO harness was removed. Desktop UI automation
  uses this Playwright/CDP path only.
- Backend coverage uses plain `fetch` against `/api/*` with the token from
  `~/.codeterm-dev-playwright/config.toml`.

## Run

```bash
pnpm test:pw:lint    # fail if any Playwright test has no tier tag
pnpm test:pw:smoke   # @smoke project
pnpm test:pw:golden  # @golden project, one retry and 5min timeout
pnpm test:pw:perf    # @perf stub-provider latency budget project
pnpm test:pw:full    # all non-quarantined tests
pnpm test:pw:live-pair # bounded real-Claude orchestration launch gate
pnpm test:pw:report  # open the last HTML report
pnpm test:pw:trace -- tests/playwright/artifacts/test-results/<trace.zip>
pnpm test:pw:debug   # run the suite with Playwright Inspector
```

### Fast isolated check against an existing test console

When the named Playwright instance is already healthy, use the adopt-only runner
to run one exact spec and title without rebuilding, restarting, or tearing it down:

```bash
pnpm test:pw:one -- --file tests/playwright/suites/01-smoke/smoke.spec.ts \
  --grep "real UI renders" --project smoke
```

It verifies the named instance identity, API health, and CDP/WebView readiness
before launching Playwright. It always applies a 30-second per-test cap and a
10-minute global cap. If preflight fails, it exits nonzero and leaves every
instance untouched; use the normal harness when a clean start is actually needed.

Parallel execution is opt-in. `CT_E2E_WORKERS=N` starts `pw1..pwN` with
separate API, Vite, CDP, identifier, token, and `.codeterm-dev-pwN` state.
Playwright's stable `parallelIndex` pins each worker process to one instance.
Startup is serialized through the build gateway, which is released after each
named instance passes readiness; Windows instances use separate Cargo targets.
The harness stops every named instance and fails the run if any owned port
remains open. The golden scripts do not set this flag and remain serial.

```bash
CT_E2E_WORKERS=2 npx playwright test --config=tests/playwright/playwright.config.ts \
  --project=full --workers=2 --timeout=30000 --global-timeout=600000 \
  tests/playwright/suites/06-mem tests/playwright/suites/11-api \
  tests/playwright/suites/13-canvas/plan-view-mode.spec.ts
```

### @perf capped run

Run the perf gate with the required authoring caps via the DIRECT npx form:

```bash
npx playwright test --config=tests/playwright/playwright.config.ts \
  --project=perf --timeout=30000 --global-timeout=600000
```

Do NOT use `pnpm test:pw:perf -- --timeout=30000 ...`: pnpm forwards the
post-`--` flags to the script as positional test-file filters, so Playwright
treats `--timeout=30000` as a path and reports "No tests found". Only the
direct `npx playwright test ...` invocation applies the caps as flags.

The harness normalizes the `CODETERM_STUB_AGENT_CMD` replay path to absolute
against the repo root (`CT_E2E_DEV_ROOT` or the checkout root) before the daemon
env handoff, so a repo-root-relative value like
`node tests/playwright/fixtures/stub-agent/replay.mjs` resolves correctly even
though the agent PTY runs with cwd `src-tauri`.

On Windows PowerShell, start/restart dev with `.\build.ps1 dev --wait`.
Do not call plain `bash ./build.sh ...` from PowerShell; it can resolve to
WSL. Use `build.ps1` or Git Bash directly.

The harness owns its dev lifecycle. Do not start the interactive 7686 instance
for Playwright. To select another isolated lane, set `CT_E2E_INSTANCE` and
`CT_E2E_PORT`; ports 7685 and 7686 are rejected. Global teardown stops only the
named Playwright instance.

**There is no CI for this suite — by decision, not omission** (hosted
runners lack the claude CLI auth and a verified WebView2 desktop; a
self-hosted runner was considered and rejected). The contract:

> **Run the suite locally before opening a PR.** `@smoke` minimum for
> any change; the full suite (`pnpm test:pw:full`, ~3 min, a handful of
> trivial sonnet calls) for changes touching the daemon API,
> multiplexer, mesh, mem, agents, or the frontend shell.

Windows is required for the desktop UI-driving tests (CDP/WebView2 — see
the tooling section); on macOS/Linux run API/unit layers and say so in the PR.

`pnpm test:pw` resolves to `playwright test -c tests/playwright`; the tiered
scripts select the `smoke`, `golden`, or `full` projects from the config. If your
shell wraps test runners and compresses output (rtk does), get raw
per-test output with:

```bash
node node_modules/@playwright/test/cli.js test -c tests/playwright --reporter=list
```

## Replay workflow

Playwright writes post-mortem artifacts under `tests/playwright/artifacts/`:

- `html-report/` contains the last interactive report. Open it with
  `pnpm test:pw:report`.
- `test-results/` contains per-test attachments. Failed tests retain screenshots,
  videos, and a `trace.zip`; retried `golden` and `full` tests retain retry
  traces too.

To replay a failure, open the HTML report first, expand the failed test, and use
the trace attachment link. To open a trace directly from the shell, pass the zip
path after the script separator:

```bash
pnpm test:pw:trace -- tests/playwright/artifacts/test-results/<test-output>/trace.zip
```

Global setup (`helpers/global-setup.ts`) reuses only a dev console whose health
identity matches the named Playwright instance, API port, and derived CDP port.
Otherwise it runs `./build.sh kill && ./build.sh dev --wait` with that identity.
The shared interactive dev console cannot satisfy this check. In parallel mode,
global setup clean-starts each named instance before workers launch and owns
their teardown after the last test.

## Layout

```
tests/playwright/
├── playwright.config.ts    serial default; CT_E2E_WORKERS opts into isolated workers
├── helpers/
│   ├── dev-instance.ts     worker slot → named instance/ports/data dir
│   ├── dev-console.ts      start/kill/health/CDP attach (build.sh only, never raw cargo)
│   ├── instance.ts         named-instance identity and shared-port refusal
│   ├── api-client.ts       typed /api/* wrapper, token from the named data dir
│   ├── reset.ts            apiReset (fast, per-test) + wipeDevState (suite-level)
│   └── global-setup.ts
└── suites/
    ├── 01-smoke/           harness proof: health, real UI renders, providers, workspace
    └── 02-multiplexer/     ws/tab lifecycle, #132 focus determinism (50x), kill/restart persistence
```

## Real pair-launch release gate

The golden agent tests deliberately use `fixtures/stub-agent/replay.mjs` and
do not launch a provider process. They cannot detect a real Claude launch that
falls back to a bare shell, or a Planner follow-up lost during provider startup.

Before release, run this bounded gate on a host with Claude CLI authentication:

```bash
pnpm test:pw:live-pair
```

It calls the real `/api/orchestration/new` path, requires tab-local session ids
for both the Orchestrator and Planner, verifies the Planner's real tab id in
the Orchestrator follow-up buffer, and fails if the replay banner appears. The
command has a 10-minute global cap; the test itself has a 5-minute cap. It does
not depend on the model obeying an injected plan-creation instruction. The pair
POST has a 90-second client cap, leaving bounded headroom above the backend's
75-second shared launch deadline while still catching the old 120-second stall.

For a failing ROG rerun, preserve the line-reporter output and collect:

```powershell
Get-Content "$HOME\.codeterm-dev-playwright\logs\codeterm.log" |
  Select-String 'orchestration-pair|agent-launch|readiness|claude' |
  Select-Object -Last 300
Get-ChildItem tests\playwright\artifacts\test-results -Recurse
```

The API failure must include either the backend's degraded-pair recovery command
or `POST /api/orchestration/new timed out after 90000ms`; a bare `fetch failed`
is insufficient evidence.

## Real-LLM strategy (Andrey's ruling, supersedes testplan §4.1)

There is **no mock provider and no HTTP intercept** — agent tests call
the real LLM, trading mock determinism for real-system fidelity:

- LLM-touching tests are `@golden`/`@nightly`, never `@smoke`; `@smoke`
  agent coverage is LLM-free (spawn validation, report IPC via plain
  shell tabs wired with `/role`).
- Assertions are **structural**, never exact-text: response/report
  exists, contains a unique marker, tab has `agent_type`, tool/report
  history shows the action — same rank-tolerance philosophy as 06-mem.
- Be parsimonious: one LLM call per test, trivial prompts, cheap model
  (`sonnet`), bounded timeouts (240s budget for spawn→report).
- Auth: the claude provider uses the host's `claude` CLI login; no API
  key env var needed. codex/opencode agent tests are deferred until
  their CLI auth on the test machine is confirmed.
- **Terminal DOM readability**: the app uses xterm's WebGL renderer —
  the canvas has no readable text. `screenReaderMode: true` (enabled in
  `useXterm.ts` for exactly this + real accessibility) maintains a
  renderer-independent DOM mirror at `.xterm-accessibility`; fully-live
  tests read agent responses from there and cross-check against
  `/api/tabs/{id}/text`.

## Mesh peer simulator (testplan §4.2)

`helpers/mesh-stub.ts` — in-process TS HTTP stub instead of the
suggested Rust binary (consumer is this harness only; no build step, no
child processes, instant mode switches). Modes: `happy` (answers all,
advertises N services), `wedged` (TCP-accepts, never responds — the
#130 scenario), `liar` (alive, but services/relay error). 04-mesh pairs
it with the real daemon via add-manual → connect → pair-accept callback
and asserts the full liveness cycle including the 2-miss hysteresis.

## Tags

Every Playwright test title must include one tier tag; `pnpm test:pw:lint`
checks this before a test can land unnoticed.

- `@smoke`: every commit; fast and deterministic.
- `@golden`: PR gate for real-agent paths; real LLM allowed, parsimonious per
  test, run with `pnpm test:pw:golden`.
- `@perf`: deterministic stub-provider latency budgets, run with the dedicated
  `perf` project.
- `@quarantine`: excluded from `pnpm test:pw:full`; document entries in
  [`QUARANTINE.md`](QUARANTINE.md).

Non-tier tags such as `@edge`, `@failure`, or `@regression` may still describe
coverage, but they do not satisfy the lint by themselves.

## Deferred lint guardrails

`pnpm test:pw:lint` also contains stricter Playwright guardrails that are
currently opt-in because the Track F and Track S cleanups have not landed on
this branch yet. Enable them with `PW_LINT_STRICT=1` after those tracks migrate
specs to fixtures and remove timing hacks.

Strict mode rejects:

- `waitForTimeout`
- `networkidle`
- Playwright action options using `force: true`
- `test()` bodies with no `expect()`
- raw `new ApiClient()` or `connectUi()` in specs instead of the fixtures layer

For targeted proof runs, set `PW_LINT_SCOPE` to a spec file or directory under
`tests/playwright/suites`.

## Web mirror (12-web)

12-web drives the **web mirror** (served at the daemon root) in a **launched
msedge browser** — a real cross-origin browser context, unlike the other
suites' CDP attach to the Tauri WebView. Key facts:

- The SPA is the same React bundle; `src/lib/bridge.ts` maps `invoke()`
  to REST and `listen()` to the `/api/events/stream` SSE in browser mode.
- Auth is per host: the token gate (`WebGate`) stores one token per
  peer in localStorage; the dev token comes from `~/.codeterm-dev/`.
- Peer switching is exercised against the `MeshStub` running in
  `webToken` mode — it answers the SPA's direct API calls with canned
  workspaces, so a switch is provable end-to-end (stub request log +
  stub workspace rendered + clean switch-back).
- The browser terminal renders WebGL (no readable DOM); tests that read
  terminal text load `/?a11y=1` — a runtime screen-reader opt-in.
- The daemon serves the browser UI from `dist/` at `/` — run `pnpm build` after
  frontend changes or the suite tests a stale bundle.

### Driving attention state (`pending_prompt`) deterministically

Tests that need a tab in the waiting/prompt state (e.g. the RPL-4/GATE-4
prompt-pill test in `suites/19-web-stability/web-stability.spec.ts`) must get
there with no LLM and no timing luck. Use the real Claude-hook endpoint with
an explicit body, which builds a genuine `PendingPrompt` through the
production hook path (`hook_routes.rs::hook_permission`) instead of poking
client state:

```ts
await api.post(`/tabs/${tabId}/hook/permission`, {
  question: "Allow this action?",
  options: [{ label: "Yes", value: "1" }, { label: "No", value: "2" }],
});
// ... assert on GET /api/mux/state tabs[id].pending_prompt or the prompt pill ...
await api.post(`/tabs/${tabId}/prompt/clear`, {});
```

This is the canonical driver for attention state in any future test — do not
poke `pending_prompt` via client-side state or wait on a real agent to hit a
real permission prompt. A hook-driven prompt is immune to the routine
per-chunk PTY output scan clearing it (see `agent/prompt.rs`'s `explicit`
flag) — it persists under continued output until `/prompt/clear` or a
provider's own structured prompt supersedes it, which is what makes it safe
to use on a plain, still-producing-output shell tab.

Known web-mode limitations (honest rejects, sweep-audited via
`scripts/web-bridge-codemod.mjs`-era inventory): native clipboard image
paste / temp-file save, `restart_app`, `mesh_exit` / `mesh_set_access`,
provider install-in-tab, custom-theme delete, service probe/smart-add
inspect, and `spawn_pty` for tabs that have no server-side PTY (all
server-spawned tabs do). Each throws a clear
"command not available in browser mode" instead of failing silently.

## Hard rules

- **Prod guard**: `reset.ts` refuses to touch any path not under
  `.codeterm-dev` — the harness must never contaminate `~/.codeterm/`.
- Restart-persistence tests call `POST /api/session/save` before killing
  the daemon: `build.sh kill` is a force-kill (taskkill //F), so the
  close-to-tray save never runs and the 30s auto-save timer makes young
  state racy otherwise.
- Suites assert **UI DOM state**, not just API JSON, wherever the action
  has a user-visible effect.

<!-- revision: 8 -->
