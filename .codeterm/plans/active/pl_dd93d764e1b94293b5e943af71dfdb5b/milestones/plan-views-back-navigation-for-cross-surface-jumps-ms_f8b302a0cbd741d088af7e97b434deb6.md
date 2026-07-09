---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f8b302a0cbd741d088af7e97b434deb6
handle: plan-views-back-navigation-for-cross-surface-jumps-ms_f8b302a0cbd741d088af7e97b434deb6
title: status_update
---

STATUS: in_review
CHANGES:
- src/components/Canvas/planView/navStack.ts (NEW) — pure push/pop/canGoBack module, cap=20
- src/components/Canvas/planView/navStack.test.ts (NEW) — 10 unit tests: push/pop/cap/reset/scroll
- src/components/Canvas/planView/planNavBack.test.tsx (NEW) — 9 component tests: button visibility, Alt+Left, Esc semantics, round-trip, scrollCaptureRef
- src/components/Canvas/planView/PlanDetailOverlay.tsx — back button (ChevronLeft, visible only when canGoBack), Alt/Cmd+Left in onKeyDown, restoreScroll prop, scrollCaptureRef prop
- src/components/Canvas/planView/PlanListView.tsx — navStack state, pushCurrentDetail helper, openDetailTask/openDetailSection push to stack, handleBack pops+restores scroll, reset on plan switch/close
- src/components/Canvas/planView/PlanBoardView.tsx — same navStack pattern as PlanListView

BUILD/TYPECHECK/TESTS:
- npx tsc --noEmit: TypeScript: No errors found
- navStack.test.ts: 10/10 passed
- planNavBack.test.tsx: 9/9 passed
- PlanDetailOverlay.test.tsx: 15/15 passed (all existing tests green)
- cargo check + clippy: passed (pre-push hook)

BRANCH: lane/plan-back-navigation
SHA: f492753a

FOLLOWUP:
- Keyboard shortcut (Alt/Cmd+Left) is wired into the panel onKeyDown handler so it only fires when the overlay panel or a child has focus — consistent with Esc behavior. The original doc-level approach had issues with happy-dom's event dispatch in tests.
- dist/ and src-tauri/bundled-cli/codeterm stub created to satisfy pre-push cargo check in this worktree; these are gitignored build artifacts, not committed.
- The back nav stack resets both on plan switch AND on overlay close (full close via X or Esc), so closing and reopening a detail starts fresh.
