---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_e220fa3f2ac44f52b3cbd32cb004e453
handle: w14-round4-ui
title: 'Wave 14 — Round-4 UI: flat list, slide-over, plan document'
---

Wave 14 (round-4 UI, loop iteration 3/3 of the visual loop): flat status list, task slide-over, plan-first header, plan document.

Areas: PlanListView + buildPlanListTree (flat status groups + belonging chips — grouping model replaced again, alignment grid stays); new PlanTaskSlideOver (right panel ~40-45%, replaces fullscreen overlay for TASK kind only; PlanDetailOverlay keeps plan/spec document style); PlanContextHeader (plan-first + plan-title -> plan document); new PlanDocumentView (goal + sections as document, per-section relation chips to spec sections and tasks); spec-doc coverage header bug fix (uses plan-state coverage source).
Verification: npm run test:unit; Playwright: flat-list order, chip navigation, slide-over open/close with left surface live, plan document relation chips; fixture screenshots for user acceptance.
