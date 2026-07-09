---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_3b0d4ec22aee4e2191f288d404680fa7
handle: render-task-freshness-relative-time-on-the-card-ms_3b0d4ec22aee4e2191f288d404680fa7
title: status_update
---

Integrated the task freshness lane into the hot checkout.

Verified by Orchestrator:
- npx tsx src/components/Canvas/cards/taskFreshness.test.ts: passed
- npx tsx src/components/Canvas/cards/cards.test.tsx: 12 passed, including W0 and W3 freshness rendering
- git diff --check: passed

Notes:
- Freshness uses status_changed_at first, then updated_at, then created_at.
- The integration test was adjusted to use Date.now() so it remains stable after 2026.
- Playwright was not run for this narrow card-level display change; keep canvas-worlds pass for the later combined UI gate.
