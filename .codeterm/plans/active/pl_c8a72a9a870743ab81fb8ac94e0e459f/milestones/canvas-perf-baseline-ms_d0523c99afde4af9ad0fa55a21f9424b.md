---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d0523c99afde4af9ad0fa55a21f9424b
handle: canvas-perf-baseline-ms_d0523c99afde4af9ad0fa55a21f9424b
title: status_update
---

Accepted by orchestrator. Integrated commits d459fd87/c2e43a92/c987ba7a. Gates: npx tsc --noEmit passed; dev 7686 build c987ba7a; CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @canvas-perf-baseline --retries=0 --workers=1 passed. Baseline: 100 nodes pan/zoom p95 16.7/16.7ms; 500 16.7/16.7ms; 1000 16.7/16.8ms; longtasks 0.
