---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4cfc24536f8441a79ef4715415657c95
handle: bug-stabilize-canvas-layout-and-camera-when-tasks-are-added-ms_4cfc24536f8441a79ef4715415657c95
title: status_update
---

STATUS: in_review
CHANGES: src/components/Canvas/layout/taskCloud.ts, src/components/Canvas/layout/intraPlan.ts, src/components/Canvas/CanvasView.tsx, src/components/Canvas/layout/layoutStability.test.ts @ 680b06f5 (pushed lane/layout-stability)
BUILD/TYPECHECK/TESTS: layoutStability.test.ts 5/5, taskCloud.test.ts 34/34, layout.test.ts 42/42, cloudFlush.test.ts 9/9, bandedLayout.test.ts 17/17
FOLLOWUP: Ready for orchestrator merge into feat/canvas-updates + ./build.sh check + golden Playwright. Fixes: (1) multi-section blob gravity compaction → append-stable packCloud — unrelated sections 0px shift on insert; (2) intraPlan pinned plan at framePadding — no global shift on bounds growth; (3) removed layout.bounds from auto-fit effect deps. Mutated-section siblings may move <40px when footprint changes — within acceptance.
