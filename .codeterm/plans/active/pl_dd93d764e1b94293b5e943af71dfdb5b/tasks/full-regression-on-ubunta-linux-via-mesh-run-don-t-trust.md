---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_d4105813a89a4eb69b1e0510cd463ba9
handle: full-regression-on-ubunta-linux-via-mesh-run-don-t-trust
title: Full regression on ubunta (Linux) via mesh — run, don't trust
status: done
---

Run the FULL regression suite on mesh peer ubunta (paired, online). Checkout feat/canvas-updates at the same recorded integration sha as the ROG lane. Steps: (1) ./build.sh check, then Rust tests through scripts/test-capped.sh with narrow per-module filters (never bare cargo test; one run at a time — shared cargo lock). (2) ./build.sh dev --wait on 7686 (or CODETERM_INSTANCE=<name> with own port if taken), verify /api/health build sha. (3) Playwright FULL project in web-mode (Linux launches its own Chromium against /web/ — install chromium first: npx playwright install chromium) with --timeout=30000 --global-timeout=600000. (4) Linux-specific: PTY behavior under native pty, .deb/.AppImage build sanity if cheap (cargo check for the bundle target is enough — full bundling belongs to the release farm). OUTPUT CONTRACT identical to the ROG lane: every failure = stride (test, error, repro, suspected area) + cheap-model-ready fix task; zero failures = counts + logs attached; every skipped suite listed with reason.
