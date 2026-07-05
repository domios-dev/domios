---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_c32790f0b09740cf89a48d11f1c650e0
handle: task-card-status-branch-layout-ms_c32790f0b09740cf89a48d11f1c650e0
title: status_update
---

STATUS: done
CHANGES: src-tauri/src/service/plan/mod.rs — open_task_count now uses t.status.counts_as_open_work() (canonical predicate, excludes Parked+Done+Cancelled); src/components/Canvas/workspacePlanEntry.test.ts — new test 'open_task_count reflects counts_as_open_work semantics: excludes done, cancelled, parked' covering 3 cases (mixed statuses shows only open count, all terminal/parked falls through to sections, singular)
BUILD/TYPECHECK/TESTS: workspacePlanEntry.test.ts 21/21 green; WorkspacePlanEntry.test.tsx 7/7 green; git diff --check clean. Broad gates deferred to orchestrator per resource note.
