---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_22eb893770ca400e8d97b47144e910fd
handle: task-card-metadata-design-pass-labeled-chips-honest-states-clean-layout-ms_22eb893770ca400e8d97b47144e910fd
title: status_update
---

STATUS: done
BRANCH: lane/task-73ea1b9d-card-metadata
SHA: 451bfeaf24045c99732cc3d7eea66082b6d8e124
WORKTREE: /Users/rollacode/Developer/codeterm-worktrees/task-73ea1b9d-card-metadata
CHANGES: WorkingOnRowView.tsx — measure-layer chip clones no longer emit data-testid
TESTS RUN: workingOnRow.test.ts 9/9; read_lints clean
GATES REQUESTED: npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @plan-card-metadata --timeout=30000 --global-timeout=600000
FOLLOWUP: duplicate was measure row + visible row both tagging chips; pane-gone fits all chips on desktop so warn appeared twice