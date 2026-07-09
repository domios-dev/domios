---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4c69d6df49294ed39608f892d60fde41
handle: bug-keep-current-agent-task-presence-accurate-on-canvas-ms_4c69d6df49294ed39608f892d60fde41
title: status_update
---

STATUS: in_review
CHANGES: paneCurrentTask.ts(+resolvePlanExclusivePaneTasks), taskPresence.ts, useCanvasLookups.ts, useCanvasOverlays.ts, cardContent.ts, CanvasView.tsx, paneOwnedPlans.ts, taskPresence.test.ts, paneCurrentTask.test.ts @ c3dd2290
BUILD/TYPECHECK/TESTS: npx tsx taskPresence.test.ts PASS; npx tsx paneCurrentTask.test.ts PASS; tsc --noEmit skipped for Canvas (8 pre-existing plugin-sdk errors unrelated)
FOLLOWUP: Root cause — assign_task does not clear stale assigned_pane_id/lane.pane_id on other tasks, so multiple tasks showed presence for one pane. Frontend now gates via resolvePlanExclusivePaneTasks (same liveness ranking as pane strip). focusApplies no longer matches loose pane-id substrings; overlay actions prefixed focus·/stride·; task cards show labeled focus field separate from subtasks N/M. Orchestrator gate: merge lane/presence-binding into feat/canvas-updates, ./build.sh dev, manual reassignment of pane 4d479b6c — verify old task pin disappears and new task gets pin+focus without restart.
