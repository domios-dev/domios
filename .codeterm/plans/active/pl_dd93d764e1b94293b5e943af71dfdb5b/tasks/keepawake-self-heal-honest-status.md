---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a6c1056515aa408892614b6902a95032
handle: keepawake-self-heal-honest-status
title: Keepawake self-heal + honest status
status: done
---

## Problem (log-proven production incident, 2026-07-02)

Prod daemon log sequence at 19:34:07Z while the daemon (pid 82763) stayed alive and kept working:

```
19:34:07 WARN [keepawake] terminated active child pid(s): [82768, 82769]
19:34:08 WARN [plan.index] vector mirror enqueue failed: sending on a closed channel
```

That pairing is the `graceful_exit_cleanup` signature (`src-tauri/src/lib.rs:942`): keepawake children killed + registry wiped + mem `checkpoint_and_close`. But there was NO actual exit — no `[shutdown] SIGTERM received`, no `[updater]` lines, no panic entries, and the process continued spawning PTYs seconds later and is still the running prod daemon. Consequences until the user manually restarted the session ~11.5h later:

1. `keepawake status` reported `active=true` while no caffeinate held any assertion — `cleanup_active_children_for_exit` kills children but never clears the in-memory session state or the snapshot, so status lies.
2. The Mac could sleep all night despite the user having armed keepawake manually.
3. mem vector-mirror channel was closed in a live daemon (separate breakage of the same phantom-exit event, noted here as evidence; plan.index writes silently failed after that point).

Restore-on-restart itself is proven working (4 restarts on 2026-07-02 each logged `resuming indefinite session from snapshot`) — the bug is the phantom exit-cleanup plus the absence of self-healing.

## Fix — three layers (TDD)

1. **Keepawake supervisor / self-heal.** The keepawake module must not trust a single spawn: a periodic check (or child-exit wait) that notices the session is active but the child is dead → respawn the child and log a WARN. This makes keepawake resilient to ANY killer (phantom cleanup, external kill -9, crashed caffeinate), not just this trigger.
2. **Honest status.** `keepawake status` must verify liveness of the recorded child pid (and on macOS optionally the assertion) instead of reporting in-memory belief. Dead child + active session → report degraded/inactive truthfully (and trigger the supervisor respawn).
3. **Find and gate the phantom-exit trigger.** Identify what invoked exit-cleanup in a live daemon at RunEvent::Exit level on macOS (candidates: Tauri Exit event firing on last-window-close/Cmd+Q route that is later not honored; crash-hook path). Gate `graceful_exit_cleanup` so it only runs when the process genuinely terminates, or make it clear session state (stop semantics) instead of leaving active-but-childless state.

## TDD order

- RED: unit test — session active + child pid dead → status reports not-alive/degraded (current code would report active).
- RED: unit test — supervisor tick with active session and dead child respawns exactly one child and re-registers it.
- RED: test — cleanup_active_children_for_exit clears session state/snapshot consistency (no active-in-memory + empty-registry state possible).
- GREEN: implement supervisor + honest status + cleanup consistency.
- Investigation subtask: reproduce/trace the 19:34:07 phantom RunEvent::Exit on macOS (window close route), fix or document the trigger.
