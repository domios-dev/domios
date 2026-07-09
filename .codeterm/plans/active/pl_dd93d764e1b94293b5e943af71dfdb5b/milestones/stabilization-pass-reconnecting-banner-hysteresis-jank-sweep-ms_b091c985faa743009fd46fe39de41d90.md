---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b091c985faa743009fd46fe39de41d90
handle: stabilization-pass-reconnecting-banner-hysteresis-jank-sweep-ms_b091c985faa743009fd46fe39de41d90
title: status_update
---

STATUS: in_review
BRANCH: lane/stabilization-reconnect-jank
SHA: a29ef6845ac74eed7bcc3d633c54f27059b3cec7
RE: BOUNCE — Playwright scenario 2 now PASSES (fixed, ran locally against dev 7686).

ROOT CAUSE (two real bugs in my prior spec, both proven by instrumentation):
1. WRONG HARNESS: seedPerfFixtureAndOpenInlineCanvas paints the board from CLIENT-SIDE synthetic plans (CanvasView buildPerfFixturePlans) while the real /api/plans resilient query stays EMPTY -> hasDataRef=false -> reconnecting can NEVER flip. So failing the route did nothing. Fix: seed a REAL workspace+plan+task via API and open the real plan board (workspace-plan-entry pattern) so the query holds data.
2. SERVICE WORKER: the web client registers a workbox SW (src/sw.ts) that proxies /api/* via its OWN fetch() (event.respondWith(fetch(...))), which BYPASSES page.route entirely. Instrumentation proved it: requests appeared in page.on('request') but routeHits=0 and statuses stayed 200 under a supposed abort. Fix: open the test context with serviceWorkers:'block' (added passthrough to openCanvasWebPage/CanvasWebInit) so fetches run on the page thread where route interception works. Confirmed swController=false after.
Also severed /api/events/stream in scenario 2 so bridge-driven refetches can't inject a surprise 2nd failure between kicks (that was a flake source: 4/5 then 1 fail; after severing SSE, 5/5 + 2/2 stable).

MECHANISM: route.fulfill 503 on /api/plans. kick1 -> full-query consec=1 -> banner absent (single failure silent, verified reconnect=0). kick2 -> consec=2 -> banner shows (reconnect=1). restore+kick -> success -> clears. Kicks via window 'codeterm-web-boot-changed' (the hook's refetch trigger); run(true) preserves the consecutive-failure counter, only success resets it.

CHANGES: tests/playwright/suites/13-canvas/canvas-reconnecting-banner.spec.ts (rewritten to real-plan harness), tests/playwright/helpers/canvas-web-page.ts (+serviceWorkers passthrough), docs/spikes/reconnecting-banner-stabilization.md (rev 2, harness findings).

VERIFIED LOCALLY (dev 7686, chromium, --retries=0):
- both scenarios: 2 passed, repeated 2x.
- scenario 2 alone: 5/5 after SSE-sever fix.
- tsc: my files clean (0 errors). NOTE: 9 pre-existing tsc errors exist but ALL are plugins/provider-*/git '@codeterm/plugin-sdk' module-resolution noise from the read-only symlinked node_modules (workspace pkgs unlinked) — not my code, unrelated.
No cargo, no dev restart, no build.

FOLLOWUP:
1. Orchestrator gate cmd: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden -g "@canvas-reconnecting"
2. serviceWorkers:'block' passthrough on openCanvasWebPage is reusable — any future web-mode spec that needs page.route over /api/* MUST block the SW or interception silently no-ops (routeHits=0). Consider documenting in tests/playwright/README.
3. Unit gates unchanged and green (useResilientQuery 5/5, fetchResilience 6/6, planViewFetchGate 2/2, usePlans 5/5, PlansProvider 5/5).
4. Prior deferred items still stand: no explicit bridge-down OR-signal wired (bridge.ts has no connection-down event); rapid-refetch root cause likely backend sidecar watcher event frequency.
