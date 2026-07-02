---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_ce69af02d4c646f4a396a62705300171
handle: canvas-perf-baseline-ms_ce69af02d4c646f4a396a62705300171
title: status_update
---

STATUS: in_review
CHANGES: tests/playwright/helpers/canvas-perf.ts; tests/playwright/suites/13-canvas/canvas-perf-baseline.spec.ts; latest sha 8a0b2e93 pushed to origin/lane/canvas-perf-baseline. Patch after gate failure creates/focuses a temporary workspace before loading the localStorage perf fixture, so canvas mounts deterministically from General/empty dev state.
BUILD/TYPECHECK/TESTS: npx tsc --noEmit passed. CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @canvas-perf-baseline --retries=0 --workers=1 passed on dev build a5edc830. Baseline: 100 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.8/16.8/16.8ms longtasks=0/0 dropped=0/0%; 500 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.8/16.7/16.7ms longtasks=0/0 dropped=0/0%; 1000 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.7/16.8/16.8ms longtasks=0/0 dropped=0/0%. L5 thresholds emitted: all node counts pan<=16.7ms zoom<=16.7ms longtasks<=0.
FOLLOWUP: cards and pins are currently 0 on the blit/swarm renderer; the harness records them explicitly and also records swarm/live-text presence so synthetic live agents are still verified.
