---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f0ff0dbb17124c4fadab9db9dd8ee406
handle: plan-picker-per-plan-kebab-menu-archive-rename-more-ms_f0ff0dbb17124c4fadab9db9dd8ee406
title: status_update
---

STATUS: done
CHANGES: PlanSwitcher.tsx (kebab menu, archived toggle, overflow popover), usePlanSwitcherActions.ts (shared optimistic archive/rename/unarchive), PlanBoardView.tsx + PlanListView.tsx (wire onAction), plansApi.ts (archive/unarchive/rename helpers), globals.css (switcher menu styles), PlanSwitcher.test.tsx + PlanBoardView.test.tsx + PlanListView.test.tsx
BUILD/TYPECHECK/TESTS: git diff --check pass; npx tsx PlanSwitcher.test.tsx 12/12 pass; PlanBoardView.test.tsx 14/14 pass; PlanListView.test.tsx 13/13 pass; broad build not run
FOLLOWUP: lane rebased onto origin/feat/canvas-updates (8e85b7dc); commit bca3a1f0 on lane/plan-picker-kebab-menu. Open plan document menu item only renders when docUrl is set on options (not wired from Plan yet — no file_path on full Plan type).
