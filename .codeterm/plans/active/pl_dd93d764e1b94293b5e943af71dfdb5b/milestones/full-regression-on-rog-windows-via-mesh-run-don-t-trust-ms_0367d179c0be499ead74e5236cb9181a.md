---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0367d179c0be499ead74e5236cb9181a
handle: full-regression-on-rog-windows-via-mesh-run-don-t-trust-ms_0367d179c0be499ead74e5236cb9181a
title: Phase C Windows Playwright lint blocker addressed. ROG at 819c887e reported TS unit PASS, then pnpm test
---

pw:lint FAIL on 92 pre-existing Playwright discipline violations (missing tags, waitForTimeout/force/raw ApiClient) across legacy suites. Added exact baseline gate in 0a8adb58: current violations are allowed, new violations and stale fixed entries fail. Local pnpm test:pw:lint PASS; pre-push cargo check/fmt/clippy PASS; pushed origin/feat-canvas-updates. ROG instructed to move C:\ctpc to 0a8adb58, rerun lint, then continue native WebView2 Playwright/regression if green.
