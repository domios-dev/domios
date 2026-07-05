---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6d3b4375f8fc47e489460467d300b0a5
handle: stabilization-pass-reconnecting-banner-hysteresis-jank-sweep-ms_6d3b4375f8fc47e489460467d300b0a5
title: status_update
---

STATUS: in_review
BRANCH: lane/stabilization-reconnect-jank
SHA: f3f8c2bea0e4705a71d676ca293cdaa2e8660a43
CHANGES: src/lib/useResilientQuery.test.tsx (+2 tests: anti-flap counter-reset, cold-start no-banner), src/lib/fetchResilience.test.ts (+3 tests: non-transient 4xx/5xx, status passthrough, empty=transient), tests/playwright/suites/13-canvas/canvas-reconnecting-banner.spec.ts (new), docs/spikes/reconnecting-banner-stabilization.md (new audit)
VERIFIED: state machine satisfies contract — banner requires >=2 consecutive STALE failures, clears on first success, healthy stale refetch silent, cold-start failure uses blocking error not banner, intervening success resets counter (no flap). All 5 tsx gates green (useResilientQuery 5/5, fetchResilience 6/6, planViewFetchGate 2/2, usePlans 5/5, PlansProvider 5/5). tsc --noEmit -p . = 0 errors (node_modules symlinked to warm checkout). NO cargo, NO dev restart, NO build.
JANK SWEEP: no sub-second polling in plans path — usePlans/PlansProvider fully event-driven (plan-changed/plans-loaded/onResync/boot-changed); only timer is backoff retry 500ms->15s jittered. usePlanDrift defers initial fetch to requestIdleCallback. useCanvasOverlays 1600ms tick is demo-gated (off by default). Banner is outcome-driven (reconnecting) not lifecycle-driven (loading).
FOLLOWUP:
1. Playwright (orchestrator gate, dev healthy on 7686): npx playwright test --config=tests/playwright/playwright.config.ts --project=golden -g "@canvas-reconnecting"
2. DEFERRED (out of FE boundary): explicit bridge-down OR-signal NOT wired — src/lib/bridge.ts has no connection-down event; banner relies on >=2-failure path (~1s). Wire immediate reconnecting on bridge-down if such an event lands.
3. DEFERRED (backend): rapid-refetch ROOT cause is likely a sidecar watcher emitting plan-changed/plans-loaded at high freq — FE now absorbs silently+cheaply, but coalescing at source would cut churn further. Flag for backend lane.
4. Left node_modules symlink in worktree (gitignored, read-only to warm checkout).
