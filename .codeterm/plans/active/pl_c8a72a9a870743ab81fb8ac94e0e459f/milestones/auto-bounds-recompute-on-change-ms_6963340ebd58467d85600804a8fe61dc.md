---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6963340ebd58467d85600804a8fe61dc
handle: auto-bounds-recompute-on-change-ms_6963340ebd58467d85600804a8fe61dc
title: status_update
---

Integrated auto-bounds-recompute: added structural layout input revisioning, spec-section overlay for layout inputs, and CanvasView memo keys so bounds recompute on in-place plan mutations and async spec-section loads without render loops. Also fixed stale layout.test fixture to set primary_spec_id for spec-node expectations. Verified layoutInputRevision.test 4/4, CanvasView.test.tsx 7/7, layout.test.ts 42/42, npx tsc --noEmit, and CODETERM_API_PORT=7686 ./build.sh check.
