---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_14a3c3b68ccd4fd2aad771a6a3a9edbf
handle: full-regression-on-rog-windows-via-mesh-run-don-t-trust-ms_14a3c3b68ccd4fd2aad771a6a3a9edbf
title: Phase C Windows TS-unit failure addressed. ROG at ba608104 passed build.ps1 check, ConPTY, plugin-tests, then scripts/ts-unit-tests.sh failed only src/lib/webForks.test.ts due fork inventory line-number drift plus one new web cache persist fork. Fixed locally in 819c887e (test
---

refresh web fork inventory), verified npx tsx src/lib/webForks.test.ts => 51 passed / 0 failed, pre-push cargo check/fmt/clippy passed, pushed origin/feat-canvas-updates. ROG instructed to move C:\ctpc to 819c887e, rerun Git Bash scripts/ts-unit-tests.sh, then continue Playwright if green.
