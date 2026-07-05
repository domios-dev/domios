---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_238b65bd5561408aba8bf7b5c888057a
handle: task-lane-watchdog-tie-in-progress-truth-to-live-pane-mesh-state-ms_238b65bd5561408aba8bf7b5c888057a
title: decision
---

USER FRAMING PINNED VERBATIM (round-62, clarification — this overrides any drift toward auto-transitions): agents moving the core statuses (todo/in_progress/in_review/blocked/done) is CORRECT and stays untouched — that is not the problem being solved. The watchdog answers ONE question: 'is the assignee actually working?' — an ORTHOGONAL observed-liveness dimension rendered NEXT TO the agent-set status, never replacing it. Staleness thresholds apply to the OBSERVED signal (executor idle/silent too long), not to status age itself: a task may sit in_progress for days legitimately while its executor observably works. Any implementation that auto-moves core statuses violates this task's intent and bounces.
