---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_bbb6d489c1204f1c923763a0bb8cd4c0
handle: allow-orchestrators-to-own-plans-while-working-a-task
title: Allow orchestrators to own plans while working a task
status: done
---

Implement dual-role semantics for CodeTerm planner panes.

Context:
An Orchestrator pane must be able to own an execution plan and also be assigned to a concrete task in that plan, e.g. review a worker task. These are separate relations:
- Plan.owner = orchestration ownership / plan heartbeat / owner nudges.
- Task.assigned_pane_id or lane.pane_id = current task execution/review presence.

Problem:
Current nudge logic can treat an Orchestrator with a task assignment as a Worker because report-pending task binding wins over owned-plan binding. Delayed reminders can also prepend worker charter to an owner pane if the same pane has a task binding. This is wrong: an Orchestrator with a current task must still receive orchestrator heartbeat/state-reconciliation guidance and must not receive the Worker "NEVER spawn agents" charter.

Scope:
- Make owner-plan semantics and current-task semantics coexist.
- For role=Orchestrator and owned execution plan with open work, owner reminder/heartbeat must win over worker reminder even if task_binding exists.
- Worker charter prepend must no-op for panes that own an active execution plan with open work.
- Preserve task binding/env/report fallback: the Orchestrator can still have CODETERM_TASK_ID for the task it is personally reviewing/doing.
- Add focused tests in plan_reminder_nudge and worker_charter.
- Review existing local dirty draft if present, but do not trust it blindly.

Acceptance:
- Orchestrator pane that owns plan and is assigned task receives owner heartbeat/nudge, not worker charter.
- Worker pane that is assigned task and does not own plan still receives worker charter/nudge.
- Existing owner-only and worker-only tests continue to pass.

Tests:
- cargo test --manifest-path src-tauri/Cargo.toml service::hierarchy::plan_reminder_nudge --lib -- --nocapture
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::worker_charter --lib -- --nocapture
- cargo check --manifest-path src-tauri/Cargo.toml
