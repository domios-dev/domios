---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_30e82b9441c349398987bd2306025ada
handle: plan-lineage-and-ghost-links-ms_30e82b9441c349398987bd2306025ada
title: status_update
---

Accepted lineage lane. Integrated plan-plan lineage edges, archived predecessor and archived linked-target ghost cards, cross-frame rendering, lineage input revision tracking, and extracted CanvasEdges/CanvasNodeCard so CanvasView is 961 lines. Fixed integration issues during gate: workspace bounds now use world-space boxes, archived link targets ghost to the right of the live plan, SVG arrow marker no longer pollutes path geometry audits. Gates: npx tsx src/components/Canvas/layout/lineage.test.ts (16 passed); npx tsx src/components/Canvas/layout/layout.test.ts (42 passed); npx tsx src/components/Canvas/layout/bandedLayout.test.ts (17 passed); npx tsc --noEmit passed; CODETERM_API_PORT=7686 ./build.sh check passed; CT_E2E_PORT=7686 npx playwright test --config=tests/playwright/playwright.config.ts --project=golden --grep @content-lod passed. File-size guard: this lane reduced CanvasView below 1000; script still reports unrelated pre-existing oversized files and src-tauri/src/mem/store/tests.rs growth from other work.
