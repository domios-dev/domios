---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_117f86dbb18240fea62fd243f392c036
handle: perf-decouple-live-agent-status-churn-from-canvas-render-ms_117f86dbb18240fea62fd243f392c036
title: status_update
---

Accepted: live agent status/status-only churn no longer invalidates canvas layout or presence lookup rebuild keys. Integrated presenceFingerprint helper, removed task.status from layoutInputRevision, and added focused regression tests. Verified in hot checkout: layoutInputRevision.test 5/5, useCanvasLookups.test 2/2, CanvasView.test 6/6, npx tsc --noEmit, ./build.sh check, Playwright @canvas-presence/@workspace-plan-entry/@canvas-figma-fixtures all passed.
