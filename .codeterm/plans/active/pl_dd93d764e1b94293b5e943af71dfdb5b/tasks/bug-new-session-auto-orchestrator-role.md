---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8d89f3f6d6724e52bab7b9412f4d9da8
handle: bug-new-session-auto-orchestrator-role
title: New session tabs must start without orchestrator role
status: done
---

Fix the bug where a normal user-created new session/tab immediately appears as an Orchestrator. Expected behavior: a user-created new session has no role by default. The user can explicitly promote/select Orchestrator later from the tab's three-dot menu/role action, and that explicit action should still deliver the orchestrator onboarding prompt. Do not remove explicit orchestrator spawn/onboarding paths. Find where role is defaulted during new session/agent/tab creation and make the default role null/none for ordinary new sessions. Add tests around the new-session path and role display/metadata so ordinary sessions do not render the orchestrator marker/star/role, while explicit role selection still works.
