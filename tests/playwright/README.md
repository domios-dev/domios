# Playwright e2e — live dev console

Drives the **real** CodeTerm dev console (Tauri, port 7686), not a browser
mock of it.

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
pnpm test:pw                 # whole suite
pnpm test:pw --grep @smoke   # smoke only
```

`pnpm test:pw` resolves to `playwright test -c tests/playwright`. If your
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

## Tags

`@smoke` every commit · `@golden` PRs · `@edge` / `@failure` nightly ·
`@regression` locks a specific fix (named issue in the test title).

## Hard rules

- **Prod guard**: `reset.ts` refuses to touch any path not under
  `.codeterm-dev` — the harness must never contaminate `~/.codeterm/`.
- Restart-persistence tests call `POST /api/session/save` before killing
  the daemon: `build.sh kill` is a force-kill (taskkill //F), so the
  close-to-tray save never runs and the 30s auto-save timer makes young
  state racy otherwise.
- Suites assert **UI DOM state**, not just API JSON, wherever the action
  has a user-visible effect.
