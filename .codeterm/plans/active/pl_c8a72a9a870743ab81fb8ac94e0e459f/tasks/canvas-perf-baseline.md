---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_89c0b6975ac24f2da449ccb67c204edd
handle: canvas-perf-baseline
title: L1 1.6.2 canvas perf harness + baseline
status: done
---

Build deterministic Playwright perf probes for 100/500/1000 canvas nodes plus synthetic live agents. Capture rAF frame intervals including p95, longtask count, world-card DOM count, presence pin count, and optional dev-only render counters. This lane must not change product behavior; it defines baseline and failing thresholds for L5.
