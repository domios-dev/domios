---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_232a29f3afcd43e181f64077f10dfc0c
handle: workspace-plan-entry-and-focus-to-item-navigation-ms_232a29f3afcd43e181f64077f10dfc0c
title: status_update
---

Accepted and integrated workspace Plan entry + focus-to-item navigation.

Gates run by Orchestrator in hot checkout:
- npx tsc --noEmit (passed)
- npx tsx src/components/Canvas/workspacePlanEntry.test.ts (11/11 passed)
- npx tsx src/components/Canvas/CanvasButton.test.tsx (2/2 passed)
- git diff --check (clean)
- line counts checked: CanvasView.tsx 997, Sidebar.tsx 966, new helper/components under cap
- CODETERM_API_PORT=7686 ./build.sh check (passed)

Notes:
- Workspace tree now has a derived Plan slot for active plans relevant to workspace panes.
- Pane focus chip opens workspace Plan canvas and passes pane/item focus target through canvasFocusStore.
- Full Playwright visual gate not run in this slice; component/unit/type gates covered derivation and routing.
