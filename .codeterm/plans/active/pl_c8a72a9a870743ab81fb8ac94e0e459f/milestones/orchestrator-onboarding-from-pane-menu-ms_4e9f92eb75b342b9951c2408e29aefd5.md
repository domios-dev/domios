---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4e9f92eb75b342b9951c2408e29aefd5
handle: orchestrator-onboarding-from-pane-menu-ms_4e9f92eb75b342b9951c2408e29aefd5
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: Restrict Make Orchestrator to agent_type or chatBackend panes (UI+API); deliver onboarding via Messenger::deliver reminder with enter=true; drop decorative onboard markers; split Playwright into shell-reject + agent-promote/reminder asserts
BUILD/TYPECHECK/TESTS: npx tsx src/lib/orchestratorOnboard.test.ts pass 7/7; Rust unit tests added (orchestrator_onboard) — full cargo blocked by bundled-embed in worktree
FOLLOWUP: Orchestrator should run @smoke make-orchestrator Playwright pair against dev
