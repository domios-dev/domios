# Stabilization: reconnecting-banner hysteresis + jank sweep

Root symptom: the plans board flashed "Reconnecting…" every ~500ms while
`/api/plans` returned steady 200s. This audit confirms the resilience state
machine now satisfies the calm-reconnect contract and records the jank sweep.

## Resilience state machine — verified

`src/lib/useResilientQuery.ts` drives all four banner surfaces
(`canvas-` / `mission-control-` / `plan-board-` / `plan-list-reconnecting`),
each gated by `reconnecting && itemCount > 0` in `PlanFetchReconnectingBanner`.

Contract, all confirmed by unit tests:

- **Banner only after ≥2 consecutive stale failures.** `consecutiveFailureRef`
  increments only when data is already present; `reconnecting` flips true at
  `>= 2`. A single stale failure stays silent.
- **Clears on first success.** Any successful fetch zeroes the counter and sets
  `reconnecting = false`.
- **Healthy in-flight stale refetch is silent.** A refetch over existing data
  keeps `loading` false and never touches `reconnecting` unless it fails.
- **Cold-start failure is a blocking error, not the banner.** With no data yet,
  failures surface `error` (full `PlanFetchErrorState`), never the banner.
- **Anti-flap.** An intervening success between two failures resets the
  counter, so `fail → success → fail` does not show the banner.

Tests: `src/lib/useResilientQuery.test.tsx` (5), `src/lib/fetchResilience.test.ts`
(6), `src/components/Canvas/planView/planViewFetchGate.test.ts` (2).

## Polling / jank sweep

- **No sub-second polling in the plans path.** `usePlans` and `PlansProvider`
  are fully event-driven — `plan-changed` / `plans-loaded` bridge events plus
  `onResync` and the `codeterm-web-boot-changed` window event. No `setInterval`
  on the fetch path. Backoff retry is the only timer (500ms→15s, jittered).
- `usePlanDrift` defers its initial fetch to `requestIdleCallback` (200ms
  fallback) — never competes with first paint.
- `useCanvasOverlays` 1600ms tick is gated behind `presenceAnimateDemoEnabled()`
  (demo mode, off by default) — not a steady-state re-render source.
- Banner is outcome-driven (`reconnecting`), never lifecycle-driven
  (`loading`), so an in-flight request cannot flash it.

## Deferred / out of FE boundary

- **Explicit bridge-down signal (OR-condition) not wired.** `src/lib/bridge.ts`
  exposes no connection-down event today, so the banner relies solely on the
  ≥2-failure path (~1s to appear). If a bridge disconnect event is added later,
  set `reconnecting` immediately when `itemCount > 0`.
- **Rapid-refetch root cause is backend.** If a sidecar watcher emits
  `plan-changed` / `plans-loaded` at high frequency, the FE now absorbs it
  silently and cheaply, but coalescing those events at the source would further
  cut churn. Flag for the backend/orchestrator lane.

## Playwright

`tests/playwright/suites/13-canvas/canvas-reconnecting-banner.spec.ts` — runs on
a REAL plan board (seeds a workspace + plan + task via the API), so the resilient
query actually holds data and `reconnecting` can flip:
- healthy board: banner absent across 12 sampled refetch kicks;
- route interception (`/api/plans` → 503): one failure stays silent, a second
  consecutive failure shows the banner, recovery clears on first success.

Two harness facts this spec depends on (both real bugs to avoid re-hitting):
1. **A perf/synthetic fixture is the WRONG harness** — it paints a board from
   client-side synthetic plans while the resilient query stays empty
   (`hasDataRef` false), so `reconnecting` never flips. Use real seeded plans.
2. **The web client's service worker proxies `/api/*` via its own `fetch()`,
   which bypasses `page.route()`.** The spec opens the context with
   `serviceWorkers: "block"` (new passthrough on `openCanvasWebPage`) so plans
   fetches stay on the page thread and interception takes effect. It also severs
   `/api/events/stream` during scenario 2 so bridge-driven refetches can't inject
   a surprise failure between kicks (kept the hysteresis assertion deterministic).

Verified green (both scenarios, `--retries=0`, repeated) against dev on 7686:

```bash
npx playwright test --config=tests/playwright/playwright.config.ts \
  --project=golden -g "@canvas-reconnecting"
```

<!-- revision: 2 -->
