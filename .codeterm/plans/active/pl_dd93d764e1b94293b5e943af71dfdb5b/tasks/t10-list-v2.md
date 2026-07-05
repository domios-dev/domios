---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_986b9765e68b4109abf282d46bc7972a
handle: t10-list-v2
title: 'List v2: task-first stream with wave groups'
status: done
---

List v3 — REWORKED after Planner live inspection (round 3; the round-2 direction was insufficient). Read the ROUND 3 block of spec section "List — secondary view requirements" FIRST — it is the design contract. Evidence screenshots: /tmp/planner-shots/10-List.png (current failure), plus the alignment-grid description in the spec.

Scope (structure from round 2 stands: task-first, spec tree out, groups by wave, PlanSwitcher from t9):
1. Alignment grid rows: [status dot][title flex 1-line][executor fixed][age fixed], right columns aligned across all rows; subtasks indent title column only. NO pill boxes in rows — dots + dimmed word for blocked/in_review only.
2. Typography/rhythm per spec: 32-36px rows, no per-row borders, sticky slim group headers with inline progress, 60%-opacity meta.
3. Delete the duplicated spec-root/plan-root header blocks (still rendering live) — one compact context header only.
4. Calm color budget: dots + single accent for the pinned active strip (t14 hooks in later).
5. Tests: rebuild PlanListView/buildPlanListTree tests for the grid model; Playwright list spec updated; BEFORE/AFTER screenshots with the 210-task perf fixture (harness: localStorage canvas.perfFixtureN=210) attached to the report — the after must be judged against the spec design language, and final visual acceptance is the user's.
Commands: npm run test:unit; npx playwright test -c tests/playwright --project=golden; fixture screenshots via web-mode.
