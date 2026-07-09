---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_943aa1484d49414fbcb49d44cd67f982
handle: boot-stage-events-daemon-emits-truthful-pane-spawn-progress
title: 'Boot-stage events: daemon emits truthful pane spawn progress'
status: done
---

The UI currently guesses what a booting pane is doing. Make the daemon emit stage events over the existing event bridge as launch_agent progresses: pty_spawned -> shell_ready (prompt signal fired) -> agent_launching (launch cmd dispatched) -> agent_ready (existing readiness point), plus terminal failure stages (spawn_failed with reason). One event type, payload {pane_id, stage, ts}; frontend consumes it for the BusyState labels; the dev log keeps the existing lines (events complement, not replace). Stages must come from the REAL transition points in launch.rs / pty_io (the spots the spawn-latency lane just touched — coordinate/rebase on it), never timers. Also expose the stage on the pane snapshot so late-attaching UIs (web mode reconnect) see the current stage without replaying events. Executable test plan: (1) Rust unit: launch path emits the ordered stage sequence on a mocked emitter — exact order asserted, failure path emits spawn_failed; (2) integration on dev: spawn an agent, subscribe to events (existing test helper / socket), assert all four stages arrive in order with sane deltas (<1.5s to shell_ready post reader-race fix, agent_ready < 10s); (3) snapshot test: pane snapshot carries current boot stage mid-boot.
