# Playwright — live dev console

Drives the **real** CodeTerm dev console (Tauri, port 7686), not a browser
mock of it.

For the canonical layer taxonomy and the "which layer do I write?" decision tree,
see [`docs/TESTING.md`](../../docs/TESTING.md).

## Tooling decision (spike result, 2026-06-06)

- On Windows the Tauri window is **WebView2**, which speaks the Chrome
  DevTools Protocol when the app is launched with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`.
  Playwright attaches via `chromium.connectOverCDP` — full control of the
  actual app UI, real Tauri IPC, no shimming. Verified live: the dev
  console answers `/json/version` on 9223.
- Pure browser-side Playwright against the Vite server is **not viable**:
  the frontend calls `@tauri-apps/api` `invoke()` directly with no HTTP
  fallback, so outside a Tauri WebView the UI is non-functional.
- `./build.sh webdriver` wires the app-embedded WebDriver server (port
  4445) for the existing **wdio** suite (`tests/webdriver/`). Playwright
  cannot speak classic WebDriver; wdio remains the cross-platform UI
  fallback (macOS/Linux WebKit has no CDP). This suite is Windows-first.
- Backend coverage uses plain `fetch` against `/api/*` with the dev token
  from `~/.codeterm-dev/config.toml`.

## Run

```bash
pnpm test:pw:lint    # fail if any Playwright test has no tier tag
pnpm test:pw:smoke   # @smoke project
pnpm test:pw:golden  # @golden project, one retry and 5min timeout
pnpm test:pw:full    # all non-quarantined tests
```

**There is no CI for this suite — by decision, not omission** (hosted
runners lack the claude CLI auth and a verified WebView2 desktop; a
self-hosted runner was considered and rejected). The contract:

> **Run the suite locally before opening a PR.** `@smoke` minimum for
> any change; the full suite (`pnpm test:pw:full`, ~3 min, a handful of
> trivial sonnet calls) for changes touching the daemon API,
> multiplexer, mesh, mem, agents, or the frontend shell.

Windows is required for the UI-driving tests (CDP/WebView2 — see the
tooling section); on macOS/Linux run what applies and say so in the PR.

`pnpm test:pw` resolves to `playwright test -c tests/playwright`; the tiered
scripts select the `smoke`, `golden`, or `full` projects from the config. If your
shell wraps test runners and compresses output (rtk does), get raw
per-test output with:

```bash
node node_modules/@playwright/test/cli.js test -c tests/playwright --reporter=list
```

Global setup (`helpers/global-setup.ts`) reuses a running dev console when
both `/api/health` and CDP answer; otherwise it runs
`./build.sh kill && ./build.sh dev --wait` with the CDP env var. A console
started *without* the env var cannot be adopted — the debugging port is
fixed at WebView2 creation — so the harness restarts it.

## Layout

```
tests/playwright/
├── playwright.config.ts    one worker — one live app, parallel workers would fight over focus
├── helpers/
│   ├── dev-console.ts      start/kill/health/CDP attach (build.sh only, never raw cargo)
│   ├── api-client.ts       typed /api/* wrapper, token from ~/.codeterm-dev/config.toml
│   ├── reset.ts            apiReset (fast, per-test) + wipeDevState (suite-level)
│   └── global-setup.ts
└── suites/
    ├── 01-smoke/           harness proof: health, real UI renders, providers, workspace
    └── 02-multiplexer/     ws/pane lifecycle, #132 focus determinism (50x), kill/restart persistence
```

## Real-LLM strategy (Andrey's ruling, supersedes testplan §4.1)

There is **no mock provider and no HTTP intercept** — agent tests call
the real LLM, trading mock determinism for real-system fidelity:

- LLM-touching tests are `@golden`/`@nightly`, never `@smoke`; `@smoke`
  agent coverage is LLM-free (spawn validation, report IPC via plain
  shell panes wired with `/role`).
- Assertions are **structural**, never exact-text: response/report
  exists, contains a unique marker, pane has `agent_type`, tool/report
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
  `/api/panes/{id}/text`.

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
- `@quarantine`: excluded from `pnpm test:pw:full`; document entries in
  [`QUARANTINE.md`](QUARANTINE.md).

Non-tier tags such as `@edge`, `@failure`, or `@regression` may still describe
coverage, but they do not satisfy the lint by themselves.

## Web mirror (12-web)

12-web drives the **web mirror** (`/web` on the daemon) in a **launched
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
  terminal text load `/web/?a11y=1` — a runtime screen-reader opt-in.
- The daemon serves `/web` from `dist/` — run `pnpm build` after
  frontend changes or the suite tests a stale bundle.

Known web-mode limitations (honest rejects, sweep-audited via
`scripts/web-bridge-codemod.mjs`-era inventory): native clipboard image
paste / temp-file save, `restart_app`, `mesh_exit` / `mesh_set_access`,
provider install-in-pane, custom-theme delete, service probe/smart-add
inspect, and `spawn_pty` for panes that have no server-side PTY (all
server-spawned panes do). Each throws a clear
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

<!-- revision: 1 -->
