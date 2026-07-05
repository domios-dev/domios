---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8be0a8a58f5342659e2de4a756650daf
handle: t18-plan-document
title: Plan document view + plan-first header + coverage bug
status: done
---

Plan document view + plan-first header (round 4). Read spec sections plan-document and the ROUND 4 block of context-surface first.

Steps:
1. PlanDocumentView: goal block (vector/constraints/postulates), sections as readable document (markdown bodies, nesting); reuse the spec-document rendering shell.
2. Relation line under each section title: chips for linked spec sections (-> spec document at section) and attached tasks (-> task slide-over). Both directions navigable.
3. PlanContextHeader becomes plan-first: plan title (click -> plan document), goal one-liner, progress, focus, spec chip (-> spec document). Board + List both.
4. BUG FIX: spec document header coverage ("24 unplanned / 0% covered" vs plan state 100%) — read the same coverage report plan state uses; add a regression test.
5. Tests: document render, relation chips navigation, header routing, coverage source; Playwright + screenshots.
