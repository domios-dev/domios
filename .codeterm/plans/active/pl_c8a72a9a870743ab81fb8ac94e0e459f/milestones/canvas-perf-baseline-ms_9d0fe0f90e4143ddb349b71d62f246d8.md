---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9d0fe0f90e4143ddb349b71d62f246d8
handle: canvas-perf-baseline-ms_9d0fe0f90e4143ddb349b71d62f246d8
title: status_update
---

STATUS: in_review
CHANGES: tests/playwright/suites/13-canvas/canvas-perf-baseline.spec.ts; latest sha 51fdbe25 pushed to origin/lane/canvas-perf-baseline. The spec no longer uses the shared ui fixture. It creates/focuses a temp workspace through API first, then opens a fresh webBrowser page with token + perf fixture localStorage already installed, so it does not require pre-existing panes/chat shell.
BUILD/TYPECHECK/TESTS: npx tsc --noEmit passed. Exact gate passed on dev build b783d5bc: CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @canvas-perf-baseline --retries=0 --workers=1. Baseline: 100 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.8/16.7/16.7ms longtasks=0/0 dropped=0/0%; 500 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.7/16.7/16.7ms longtasks=0/0 dropped=0/0%; 1000 nodes cards=0 pins=0 swarm=5 liveText=20 idle/pan/zoom p95=16.8/16.7/16.7ms longtasks=0/0 dropped=0/0%. L5 thresholds emitted: all node counts pan<=16.7ms zoom<=16.7ms longtasks<=0.
FOLLOWUP: Harness is now deterministic from fresh/empty dev shell state because test setup happens before page open; global setup still only requires daemon health.
