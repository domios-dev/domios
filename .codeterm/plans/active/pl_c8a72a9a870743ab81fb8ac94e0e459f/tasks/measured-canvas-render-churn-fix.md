---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_e37c5dfba2254bed89da671939695652
handle: measured-canvas-render-churn-fix
title: L5 measured canvas render-churn fix after baseline
status: done
---

After L1 lands, target the measured hot spots only. Likely areas: plan object identity churn, lookup/rollup memoization, presence rebuild churn, DOM double-paint, canvas-primary split. Must move L1 numeric metrics measurably; no subjective perf claims.
