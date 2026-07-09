---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_51f0234359f84a72a73d8c0a5ba8771f
handle: orchestrator-onboarding-from-pane-menu-ms_51f0234359f84a72a73d8c0a5ba8771f
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: Tab 3-dot Make Orchestrator menu; POST /api/panes/{id}/make-orchestrator + mux_make_orchestrator; onboarding message inject with codeterm doc orchestration / orchestrator status / heartbeat / plan task / agent spawn commands; doc alias canonicalization
BUILD/TYPECHECK/TESTS: npx tsx src/lib/orchestratorOnboard.test.ts pass; Rust orchestrator_onboard unit test added; @smoke Playwright gate in agents.spec.ts (orchestrator will run against dev)
FOLLOWUP: Orchestrator should run dev + Playwright @smoke make-orchestrator test; full cargo build needs bundled-embed on this machine
