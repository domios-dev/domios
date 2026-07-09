---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_4729907764384c6ca2ea0adf36cd0791
handle: orchestrator-wake-on-report-reliability-reports-nudges-must-wake-an-idle-ms_4729907764384c6ca2ea0adf36cd0791
title: decision
---

Autonomy mandate adopted as current orchestration discipline: do not trust push wake-ups; before going idle run plan state + plan in-progress, judge every in_review report, dispatch or park every pending task, and poll silent workers after 20 minutes. Current live focus is wake-on-report reliability in lane/wake-on-report-reliability; stale completed task records were reconciled. Loop record RPC is absent on this daemon, so loop_f585 closure evidence remains noted here for later reconciliation.
