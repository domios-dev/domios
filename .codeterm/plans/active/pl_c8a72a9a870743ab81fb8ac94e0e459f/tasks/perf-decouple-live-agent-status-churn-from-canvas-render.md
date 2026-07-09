---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_9ce801b4f45e414da814d75d5bc6b2c3
handle: perf-decouple-live-agent-status-churn-from-canvas-render
title: 'PERF: decouple live agent status churn from canvas render'
status: done
tags:
- performance
- presence
- canvas
---

Working agents update live state/status lines frequently. The canvas must not rebuild scene/layout/lookups or rerender the whole board on each agent_state tick. Audit panes/plans props and memo keys, isolate presence-relevant fields, throttle/debounce non-visual status churn, and add counters/tests proving canvas recompute is bounded under working-agent updates.
