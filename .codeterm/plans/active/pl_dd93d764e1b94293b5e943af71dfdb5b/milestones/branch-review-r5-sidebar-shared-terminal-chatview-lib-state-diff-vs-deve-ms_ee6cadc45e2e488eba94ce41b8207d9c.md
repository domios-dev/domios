---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ee6cadc45e2e488eba94ce41b8207d9c
handle: branch-review-r5-sidebar-shared-terminal-chatview-lib-state-diff-vs-deve-ms_ee6cadc45e2e488eba94ce41b8207d9c
title: review
---

R5 Sidebar/shared/Terminal/ChatView/lib review found 1 HIGH issue. Finding: src/store/multiplexer/index.tsx initializes writeGateRef with createWriteGate(false) even when buildInitialState synchronously paints cached web mux state (generation HYDRATED_GENERATION). The gate is only locked in useEffect after first paint, so cached stale tabs/panes can trigger guarded mutating calls before first live mux_get_state reconcile, risking pane-list/tab desync or ghost actions after daemon restart/peer switch. Created cheap-model fix task task_3e39b1b85b0f4afe9c795a5a63607ccb.
