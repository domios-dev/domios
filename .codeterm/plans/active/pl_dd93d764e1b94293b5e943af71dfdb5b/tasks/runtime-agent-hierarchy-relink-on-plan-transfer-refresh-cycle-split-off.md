---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fabf2386f3f7449fb1c47d53227e6507
handle: runtime-agent-hierarchy-relink-on-plan-transfer-refresh-cycle-split-off
title: Runtime agent-hierarchy relink on plan transfer (refresh-cycle split-off)
status: pending
---

TWO deliverables. (1) RUNTIME HIERARCHY RELINK ON TRANSFER: on plan ownership transfer, repoint the runtime agent-hierarchy parent edge (service/hierarchy.rs) for the bound planner + active workers to the new owner pane. Plan-store binding is already preserved by transfer(); this is the runtime hierarchy edge only. (2) REFLECTION-TRIGGER (Planner ruling, binding): key after_reflection on TEMPORAL CORRELATION with daemon-owned state — fire when a learning is recorded WHILE the reflection window is pending (the reflection silencer already tracks pending/cleared). NO magic tag; the prompt needs zero protocol awareness; recording the learning both clears the block AND arms the refresh. DELETE the tag=='reflection' path outright.
