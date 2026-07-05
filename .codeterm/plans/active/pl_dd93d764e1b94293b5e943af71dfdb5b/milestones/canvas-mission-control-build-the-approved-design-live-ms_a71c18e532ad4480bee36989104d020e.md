---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_a71c18e532ad4480bee36989104d020e
handle: canvas-mission-control-build-the-approved-design-live-ms_a71c18e532ad4480bee36989104d020e
title: testing
---

Accepted after cherry-picking e1cc30e1 as 4fc5a046; focused golden smoke passed: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden tests/playwright/suites/13-canvas/plan-view-mode.spec.ts --grep 'plan surface exposes'. Pre-push cargo check/fmt/clippy also passed.
