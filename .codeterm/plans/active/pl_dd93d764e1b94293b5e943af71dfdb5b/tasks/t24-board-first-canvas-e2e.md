---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4214dc2ce4a64fcd9168e9d14e555ca6
handle: t24-board-first-canvas-e2e
title: Board-first Playwright canvas helper + frame evidence
status: cancelled
---

Fix the e2e/perf gate broken by Board-first default. ensureCanvasBoard and related canvas helpers must explicitly switch to Canvas mode before waiting for [data-testid=canvas-view]. Produce frame-budget evidence for the 200/210-task fixture after Canvas v3 fixes land, and record screenshot artifact paths. Keep this task focused on tests/helpers/evidence, not renderer redesign.
