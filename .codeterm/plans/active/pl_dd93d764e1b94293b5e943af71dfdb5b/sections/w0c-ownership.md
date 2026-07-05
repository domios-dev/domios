---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_e6ee335d32064d1aa8019c388ea77553
handle: w0c-ownership
title: Wave 0c — Planning ownership in docs/prompts/CLI
---

Wave 0c (docs/prompts/CLI-hints lane; parallel with Waves 0/0b, low risk): encode the Orchestrator/Planner ownership boundary in the product, not chat memory.

Scope:
- docs/ORCHESTRATION.md: new "Planning ownership" subsection under Planner Profile: while a Planner owns an open planning task, requirements/blockers/evidence flow to the Planner; Orchestrator dispatches/gates/integrates and may record facts (strides, heartbeat, lane metadata) but does not synthesize graph nodes; exceptions (no Planner assigned, emergency repair) must be recorded and handed back for reconciliation; Planner reconciles foreign mutations (keep/rewrite/remove) with a correction stride. Bump revision tag.
- Prompt templates in src-tauri/prompts/ (orchestrator/worker/planner seeds incl. orchestration_toolset.md): add the same contract in the role framing.
- CLI nudge: plan/spec mutation commands (section add/update, task add/update, goal set) emit a one-line stderr hint when the target plan has an open planning-kind task assigned to a different pane than the caller: "hint: plan <id> has an active Planner lane (task <handle>); route planning input there or record a stride." Non-blocking, informational only.
- Tests: unit test for the nudge trigger condition; docs lint (revision tags).

Verification: ./build.sh check; narrowed cargo test for the nudge; manual: run a plan section add from a non-planner pane on a plan with an active planning task and observe the hint.
