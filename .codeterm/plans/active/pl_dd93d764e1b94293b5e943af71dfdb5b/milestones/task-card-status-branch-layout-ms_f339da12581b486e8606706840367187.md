---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f339da12581b486e8606706840367187
handle: task-card-status-branch-layout-ms_f339da12581b486e8606706840367187
title: status_update
---

STATUS: done
CHANGES: 10 files — src/styles/globals.css (removed order:1 from .plan-board-task-card-head, added overflow:hidden to .plan-board-task-lane), src/components/Canvas/planView/BoardTaskCard.tsx (branch span break-words→truncate), src/components/Sidebar/WorkspacePlanEntry.tsx (removed 'Plan' kind span and +N badge block), src/components/Canvas/workspacePlanEntry.ts (workspacePlanEntryHint uses open_task_count when available, falls back to task_count), src/components/Canvas/plansApi.ts (added open_task_count field to PlanIndexEntry), src-tauri/src/service/plan/types.rs (added open_task_count to IndexEntry), src-tauri/src/service/plan/mod.rs (populates open_task_count as non-done/cancelled count), + 3 test files updated/extended
BUILD/TYPECHECK/TESTS: pnpm test:unit — 131/131 TS unit test files green (incl. new layout test, open_task_count tests, +N badge removal tests). cargo check — no error[E...] compiler errors from changed files; build script pre-check failure is pre-existing codegen env issue unrelated to this work.
FOLLOWUP: Broad cargo+tsc+playwright gates deferred to orchestrator per resource note. open_task_count is now wired in both TS interface and Rust backend; hint shows remaining tasks automatically once backend upgrades deploy.
