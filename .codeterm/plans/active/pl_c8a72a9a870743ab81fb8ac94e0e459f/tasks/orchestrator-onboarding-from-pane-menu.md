---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_4224d35f70db4ff59f3c4bb7da4f5fa4
handle: orchestrator-onboarding-from-pane-menu
title: Orchestrator onboarding from pane menu
status: done
---

Make Orchestrator must be an explicit promotion flow, not a default state.

Rules:
- Newly spawned non-worker agent panes are roleless by default: no star, no Orchestrator label, no implicit role marker.
- The pane menu exposes Make Orchestrator for eligible agent/chat panes.
- Promotion stores an explicit pane RoleDescriptor(kind=orchestrator) and injects onboarding: read codeterm docs ORCHESTRATION, read the orchestrator skill, claim/create a plan, keep plan-owned heartbeats active, and spawn attached workers.
- The tab/canvas should show Orchestrator role only after this explicit promotion.
- Worker panes are attached execution children, not a role template. Their behavior comes from parent/report routing and the assigned task brief.

Acceptance:
- A fresh generic agent pane renders without Orchestrator/star role chrome.
- Clicking Make Orchestrator changes the persisted pane role and delivers onboarding text.
- Restart/restore preserves explicit Orchestrator role but does not invent it for roleless panes.
