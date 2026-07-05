---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3325ecd8ccd04fe29dad3c385e3647a9
handle: full-regression-on-rog-windows-via-mesh-run-don-t-trust-ms_3325ecd8ccd04fe29dad3c385e3647a9
title: Phase C Windows Playwright lint baseline was still red on ROG at 0a8adb58
---

92 current violations plus the same 92 stale baseline entries. Root cause: check-tags.mjs emitted OS-native Windows backslash paths while the committed baseline uses forward-slash paths. Fixed in 2cd7d81b by normalizing location() paths to '/'. Local pnpm test:pw:lint PASS; pre-push cargo check/fmt/clippy PASS; pushed origin/feat-canvas-updates. ROG instructed to move C:\ctpc to 2cd7d81b and rerun lint, then continue native WebView2 Playwright full if green.
