---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_70affef99f63423486392c221abd1e3f
handle: render-planning-lifecycle-correctly-in-canvas-list-board
title: Render planning lifecycle correctly in Canvas/List/Board
status: done
---

Make Canvas/List/Board reflect planning lifecycle correctly.

Problem observed by user:
The plan canvas/list/board can show every task as open or visually confusing after status/model changes. With PlanMode, views must distinguish planning, approved, execution, and complete state calmly.

Scope:
- Audit Canvas/List/Board surfaces for task status and new plan mode rendering.
- Ensure done/cancelled tasks are not shown as active/open and planning mode does not imply worker activity.
- Add a small mode/readiness badge or copy only if it helps; avoid noisy tab/task bubbles.
- Verify against existing quiet tab/calm UX constraints.

Acceptance:
- Done tasks render as done in Canvas/List/Board.
- Planning-mode plans do not show fake active work.
- No new animated/noisy chrome.

Tests:
- npx tsx src/components/Canvas/planView/PlanListView.test.tsx
- npx tsx src/components/Canvas/planView/PlanBoardView.test.tsx
- npx tsx src/components/Canvas/PlanViewShell.test.tsx
- Playwright visual gate if dev is running.
