---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_4ba9dfd0af3f493bafab8f10884d7927
handle: w10-list-v2
title: 'Wave 10 — List v2: task-first (round 2)'
---

Wave 10 (round-2): List v2 — task-first stream.

Scope (spec: List round-2 block):
- Remove inline spec-root/spec-section rows from the stream; spec reachable via header chip -> spec detail overlay.
- Rows are tasks grouped by plan section (wave): slim collapsible group headers (title + done/total), active groups first, done-only groups collapsed at bottom; groups auto-hide when filters empty them.
- Task row: status glyph+pill, name, executor, max one quiet metadata line; click -> detail overlay.
- Shared PlanSwitcher from Wave 9 in the header; same default-active-plan rule.
File ownership: planView/PlanListView.tsx, buildPlanListTree.ts (task-first forest rebuild), listViewState, PlanSwitcher reuse, PlanContextHeader.
Verification: npm run test:unit; updated Playwright list specs; screenshots for round-2 visual acceptance.
