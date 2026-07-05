---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_194552e780f54031a8dac76c423f0e04
handle: state-drift-recovery-39-4-canvas-hud-drift-badges-stale-in-progress-unaccepted-in-review-worker-gone-visible-on-the-board-ms_194552e780f54031a8dac76c423f0e04
title: status_update
---

Implemented in snapshot a5803c81. Evidence: usePlanDrift fetches /api/plans/:id/reconcile and TaskCard renders DriftBadge. Verified npx tsx src/components/Canvas/cards/cards.test.tsx: 10 passed, including drift badge actionable/muted/absent states.
