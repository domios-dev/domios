---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_3a78f9fd1dae42d78a5bc3bf75d7f0e3
handle: show-historical-executors-separately-from-active-presence-ms_3a78f9fd1dae42d78a5bc3bf75d7f0e3
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: committed 13083155c0c854875cbe3e581a5b349b9ec46ff6 on lane/task-history-ui. Active presence now comes only from live assignment; historical executors render separately and calmly for terminal tasks in Board/List/Detail/Card with stable data-testid hooks. Resume affordance remains hidden (no backend respawn API yet).
COMMIT_FILES: src/components/Canvas/CanvasDetailOverlay.tsx, src/components/Canvas/CanvasView.tsx, src/components/Canvas/canvasRender/DetailContent.tsx, src/components/Canvas/canvasRender/cardContent.test.ts, src/components/Canvas/canvasRender/cardContent.ts, src/components/Canvas/canvasView/models.ts, src/components/Canvas/canvasView/useCanvasLookups.ts, src/components/Canvas/planView/BoardTaskCard.tsx, src/components/Canvas/planView/PlanBoardView.test.tsx, src/components/Canvas/planView/PlanListView.tsx, src/components/Canvas/planView/boardModel.test.ts, src/components/Canvas/planView/boardModel.ts, src/components/Canvas/planView/buildPlanListTree.test.ts, src/components/Canvas/planView/buildPlanListTree.ts, src/components/Canvas/taskExecutorHistory.test.ts, src/components/Canvas/taskExecutorHistory.ts, src/components/Canvas/types.ts
BUILD/TYPECHECK/TESTS: git diff --check clean. Focused tests passed: taskExecutorHistory.test.ts, taskPresence.test.ts, boardModel.test.ts, buildPlanListTree.test.ts, cardContent.test.ts, PlanBoardView.test.tsx. tsc blocked only by existing plugin-sdk resolution errors (Cannot find module '@codeterm/plugin-sdk' in plugin files).
FOLLOWUP: backend execution_attempt/respawn API tasks still pending; enable resume button only after API lands.
