---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_36f32338f3e84cf09f8ae3f90f101d01
handle: bug-stabilize-canvas-layout-and-camera-when-tasks-are-added-ms_36f32338f3e84cf09f8ae3f90f101d01
title: status_update
---

STATUS: in_review
CHANGES: src/components/Canvas/layout/taskCloud.ts, src/components/Canvas/layout/intraPlan.ts, src/components/Canvas/CanvasView.tsx, src/components/Canvas/layout/layoutStability.test.ts @ 680b06f5
BUILD/TYPECHECK/TESTS: npx tsx layoutStability.test.ts (5/5), taskCloud.test.ts (34/34), layout.test.ts (42/42), cloudFlush.test.ts (9/9), bandedLayout.test.ts (17/17); npx tsc --noEmit has pre-existing plugin-sdk errors only in touched Canvas files
FOLLOWUP: Orchestrator should merge lane/layout-stability -> feat/canvas-updates and run ./build.sh check + golden Playwright. Root cause was (1) multi-section gravity compaction re-packed ALL section blobs on every mutation (1300px shifts) — fixed with slot-keyed packCloud; (2) intraPlan re-normalized frame origin to content min on bounds change — fixed by pinning plan at framePadding. Camera auto-fit already guarded by fitContextKey; removed layout.bounds from effect deps as belt-and-suspenders. Tasks in the MUTATED section may still move slightly (<40px) when a sibling grows — expected per acceptance.
