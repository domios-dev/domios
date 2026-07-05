---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_a65a82196cd047c9ade5a79a79c9a8ea
handle: plan-views-back-navigation-for-cross-surface-jumps-ms_a65a82196cd047c9ade5a79a79c9a8ea
title: status_update
---

STATUS: in_review
CHANGES: fix in src/components/Canvas/planView/PlanDetailOverlay.tsx (dep array), regression test in planNavBack.test.tsx
BUILD/TYPECHECK/TESTS:
- tsc --noEmit: no errors
- navStack.test.ts: 10/10
- planNavBack.test.tsx: 10/10 (includes new stale-closure test)
- PlanDetailOverlay.test.tsx: 15/15
- cargo check + clippy: passed
BRANCH: lane/plan-back-navigation SHA: 91a40e42
FOLLOWUP: Bug was useCallback([onClose]) missing canGoBack+onBack. The panel listener useEffect correctly depends on onKeyDown so it re-registers when deps change — the stale-closure test now confirms false→true transition fires onBack.
