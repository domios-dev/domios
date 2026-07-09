---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_cb0f0ec00d3b49aaa5aff713f356884e
handle: plan-board-reduce-view-switch-render-longtasks-from-responsiveness-soak-ms_cb0f0ec00d3b49aaa5aff713f356884e
title: status_update
---

STATUS: in_review
BRANCH/SHA: lane/board-soak-jank / c9302581 (base f85e98bd, has soak harness; predates mobile-header lane — no PlanContextHeader overlap)

CHANGES: src/components/Canvas/PlanViewShell.tsx only (+21/-8). Root cause: shell keeps board/list/canvas all mounted via `hidden` (to preserve state) but none were memoized, so every view-mode toggle re-rendered ALL three surface trees + companion. Fix: wrap PlanBoardView/PlanListView/MissionControlPlanSurface/PlanCompanionBubble in React.memo and stabilize the companion renderChat with useCallback, so a pure switch skips re-rendering the two off-screen surfaces. Did NOT touch PlanContextHeader/PlanBoardView internals/xterm.

BUILD/TYPECHECK/TESTS: tsc --noEmit clean. Focused tests green: PlanViewShell 4/4, PlanBoardView 16/16, PlanListView 18/18. git diff --check OK. check-file-sizes.sh OK (no new/grown files).

MEASUREMENTS: isolated dev CODETERM_INSTANCE=soak on :7702, web-mode, done-heavy fixture (board probe: done column=200 tasks, 5 cols, 3 mode tabs → fixture genuinely heavy). Ran soak-board equivalent 60s before(stash)/after. BOTH runs: longtasksOver250=0, rafGapsOver250=0, maxRafGap=19ms, ~3600 frames, 77-78 switch iters. Stats: /tmp/soak-artifacts/soak-board-stats-before.json and -after.json.

Honest read: this isolated fixture does NOT reproduce the original >250ms signal, so before/after shows no numeric delta — only that the switch path is clean and my change is a no-regression win. The board DOM is bounded (done column collapses) and the heavy model builds (buildBoardPlanModel/buildMissionControl) are memoized on stable inputs, so a PURE switch never recomputes them. The original >250ms almost certainly came from live-agent pane/status churn invalidating those memos WHILE switching — a condition the static fixture lacks.

FOLLOWUP:
1. To get a real before/after delta, the soak needs live pane churn (the measurement lane's original condition). Recommend a churn driver (spawn a few real panes emitting periodic output) merged with the switch loop; state.panes ticks are what cascade to all 3 surfaces.
2. If churn-during-switch stays the dominant cost, the bigger lever is gating off-screen surfaces' expensive recompute on visibility (skip buildBoardPlanModel/buildMissionControl while hidden) — but that touches surface internals/UX and I scoped it out to respect 'preserve UX' + avoid PlanContextHeader.
3. Machine was under heavy contention (a concurrent cold main-dev build on :7686); the soak dev took ~3 rebuild attempts due to macOS detached-PTY killing the launcher. Dev on :7702 is currently healthy.
