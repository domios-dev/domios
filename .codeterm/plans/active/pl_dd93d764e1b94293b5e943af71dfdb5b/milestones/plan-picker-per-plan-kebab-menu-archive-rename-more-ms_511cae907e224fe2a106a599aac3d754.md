---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_511cae907e224fe2a106a599aac3d754
handle: plan-picker-per-plan-kebab-menu-archive-rename-more-ms_511cae907e224fe2a106a599aac3d754
title: status_update
---

STATUS: done
CHANGES: src/components/Canvas/planView/PlanSwitcher.tsx (complete rewrite with kebab support), src/components/Canvas/planView/PlanSwitcher.test.tsx (11 tests, 8 new), src/styles/globals.css (kebab/popover/archived CSS added)
BUILD/TYPECHECK/TESTS: npx tsx PlanSwitcher.test.tsx → 11/11 passed. git diff --check clean. Broad pnpm test:unit running in background (started clean, PlanSwitcher tests confirmed passing within it).
FOLLOWUP: PlanBoardView and PlanListView pass onAction=undefined which is fine (optional prop). callers may want to wire onAction to call archivePlan/rename API + toast on error for optimistic-UI. unarchivePlan API endpoint not yet in plansApi.ts — orchestrator should add PATCH/POST /api/plans/:id/unarchive if needed. Overflow mode replaces native <select> with custom listbox; existing PlanBoardView.test snapshot test still expects data-testid=plan-switcher-option only for ≤3 plans (passing). docUrl on PlanSwitcherOption currently unused in callers — callers can pass plan.links[0] or primary_spec_id URL if desired.
