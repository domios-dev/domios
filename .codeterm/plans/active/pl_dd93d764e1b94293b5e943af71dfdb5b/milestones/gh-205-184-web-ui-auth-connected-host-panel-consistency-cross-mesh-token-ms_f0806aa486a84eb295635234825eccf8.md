---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f0806aa486a84eb295635234825eccf8
handle: gh-205-184-web-ui-auth-connected-host-panel-consistency-cross-mesh-token-ms_f0806aa486a84eb295635234825eccf8
title: decision
---

Worker fixed red peer-switch gate in lane/b27be62d-peer-switch-red at 9207542f. Scope is Playwright fixture/spec only: mesh stub now returns relay mux state and the assertion checks relay traffic instead of legacy direct web-token traffic. Focused gate reported green: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep 'peer switching' --timeout=30000 --global-timeout=300000 = 1 passed in 12.4s. Holding final task close until branch is merged/cleaned.
