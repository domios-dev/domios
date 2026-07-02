---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_ffddc406a8864e86804e60e6fd4ab45f
handle: planner-role-prompts-docs
title: Implement Planner onboarding prompts and orchestration docs
status: done
tags:
- 1.6.2
- followup
---

Implement Planner prompts/docs per plan section sec_553bdf825b194a2797e54abdd6eb3b94 (Planner role — adopted design). Runs AFTER task_216e66d lands (shares prompt files and onboarding text — sequential to avoid collisions).

1. Refine `src-tauri/prompts/planner_suffix.md` copy against the design contract (author structure top-down in the CodeTerm planner; no markdown plans; doctor before handback; report in_review via CLI).
2. `docs/ORCHESTRATION.md`: add a "Planner role" section — when the Orchestrator invokes it, the spawn command (`codeterm agent spawn claude --model fable --parent <orch-pane> --role Planner --task-file <brief>`), the handback contract, fable->opus fallback + `codeterm usage latest --provider claude` headroom check. Bump revision tag.
3. `docs/contributing/agents.md`: extend the role model with the two-layer (structural role + behavioral profile) description — a first cut landed on branch lane/planner-role-design; reconcile with it. Bump revision tag.
4. `orchestrator_suffix.md` + orchestrator_onboard.rs onboarding body: one line teaching "for plan authoring/refactoring spawn a Planner"; update the onboarding_body_lists_canonical_commands test.
5. Eval: add a planner-onboarding scenario (prompt-evaluator or evals/cli-agent) proving a cold Planner uses the planner CLI as source of truth and hands control back with in_review. Grep-gate: no doc teaches markdown plans as the primary artifact.
