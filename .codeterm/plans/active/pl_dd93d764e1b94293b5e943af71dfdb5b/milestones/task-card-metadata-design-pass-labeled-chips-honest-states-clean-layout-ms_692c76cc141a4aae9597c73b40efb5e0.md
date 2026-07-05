---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_692c76cc141a4aae9597c73b40efb5e0
handle: task-card-metadata-design-pass-labeled-chips-honest-states-clean-layout-ms_692c76cc141a4aae9597c73b40efb5e0
title: status_update
---

STATUS: done
BRANCH: lane/task-73ea1b9d-card-metadata
SHA: 86f5b14a33c7f5d76a978a1b2cd64b22119d161f
CHANGES: plan-card-metadata.spec.ts — expectWorkingOnChipValue opens +N overflow before asserting hidden chips
TESTS RUN: read_lints clean (no local Playwright)
GATES REQUESTED: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @plan-card-metadata --timeout=30000 --global-timeout=600000
FOLLOWUP: measure testid fix kept; long-branch now asserts via overflow popover when branch chip is collapsed