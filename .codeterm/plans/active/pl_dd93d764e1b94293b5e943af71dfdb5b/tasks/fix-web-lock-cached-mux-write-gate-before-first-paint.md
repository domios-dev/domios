---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3e39b1b85b0f4afe9c795a5a63607ccb
handle: fix-web-lock-cached-mux-write-gate-before-first-paint
title: 'fix(web): lock cached mux write gate before first paint'
status: done
tags:
- review-r5
- web-cache
- pane-tab-desync
---

Review R5 finding: in web mode, MultiplexerProvider builds cached mux state synchronously from loadCachedStateSync(), but writeGateRef is initialized with createWriteGate(false) and only locked later inside useEffect when generation === HYDRATED_GENERATION. React effects run after first paint, so a fast user click on cache-painted Sidebar/Terminal controls can call guardedInvoke/guardedWebJson while the gate is still open and before the first live mux_get_state reconcile. If the cached workspace/tab/pane is stale after daemon restart or peer switch, mutating calls can focus/reorder/close/spawn against dead pane or tab ids, causing pane-list/tab desync or ghost actions. Fix by deriving the initial gate locked state during render/init, e.g. createWriteGate(IS_WEB && state.generation === HYDRATED_GENERATION) or include a hasHydratedCache flag from buildInitialState, and keep IndexedDB async hydration locking before dispatch. Add tests that guarded actions are blocked immediately when sync cached state is present, then unblocked only after successful reconcile.
