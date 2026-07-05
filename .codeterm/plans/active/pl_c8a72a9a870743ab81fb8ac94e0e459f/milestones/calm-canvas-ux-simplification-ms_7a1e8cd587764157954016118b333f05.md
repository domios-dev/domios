---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_7a1e8cd587764157954016118b333f05
handle: calm-canvas-ux-simplification-ms_7a1e8cd587764157954016118b333f05
title: status_update
---

STATUS: in_review
CHANGES: lane/calm-canvas-ux-simplification @ 8253bd43 — removed panePlanMode/PanePlanFocusChip/TabContent overlay; CanvasPresenceLayer progressive disclosure (presenceReveal.ts); compact CanvasSwarmCluster; calmer globals.css + TabItem; Playwright canvas-calm-ux.spec.ts + updated presence/multiview helpers
BUILD/TYPECHECK/TESTS: npx tsc --noEmit -p tsconfig.app.json clean; npx tsx presenceReveal.test.ts + TabActivityIndicator.test.tsx pass; Playwright gates NOT run (orchestrator dev restart)
FOLLOWUP: Orchestrator should restart dev, run golden @canvas-calm-ux + @canvas-presence + @canvas-multiview-overflow; verify no regressions on workspace plan entry spec
