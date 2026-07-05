---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_85cec85bfdae43c283558ddcdd1d13c7
handle: plan-board-reduce-view-switch-render-longtasks-from-responsiveness-soak
title: 'Plan board: reduce view-switch render longtasks from responsiveness soak'
status: done
---

Investigate and reduce frontend main-thread longtasks during Plan Board/List/Canvas view switches. Evidence from responsiveness soak on eight: rAF gaps >250ms with browser longtasks during plan-board view switches; backend SLOW-IPC=0 and LOCK-HOLD=0. Likely areas: PlanBoardView, PlanViewShell, board model/render churn, expensive selectors or remounts. Coordinate with mobile compact header lane if touching PlanContextHeader-adjacent files. Deliver a narrow diff plus before/after stats from the board soak harness.
