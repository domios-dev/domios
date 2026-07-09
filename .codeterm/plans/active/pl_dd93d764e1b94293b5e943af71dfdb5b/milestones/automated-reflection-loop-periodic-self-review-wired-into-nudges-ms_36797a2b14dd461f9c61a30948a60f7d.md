---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_36797a2b14dd461f9c61a30948a60f7d
handle: automated-reflection-loop-periodic-self-review-wired-into-nudges-ms_36797a2b14dd461f9c61a30948a60f7d
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/reflection-loop @ 8524d3b7 (base origin/feat/canvas-updates 1a2a6fcd)

CHANGES (10 files, +549):
- src-tauri/src/service/plan/reflection.rs (NEW) + reflection/tests.rs (NEW): pure derivation pending_reflections(plan, every_gates) -> Vec<Reflection>. Two triggers, both piggyback existing plan-derived events (no polling): group-close (reuses milestone_gates::group_completion) and gate/bounce cadence (decision-gate asked_at + bounce-kind strides; boundary at every-N-th event). Silencer = latest heartbeat learning ts (the instructed lesson-routing primitive), which re-arms on the next close/window. ReflectionWindow{tasks_closed,bounces,gate_leaks}: gate_leaks=pending_regression_gates len (derivable), bounces=honest 0 until first-class. render_reflection() emits one-shot block.
- prompts.rs: REFLECTION_HEARTBEAT_TITLE + REFLECTION_BLOCK + reflection_block(); routes lessons to orchestrator learn / plan skill new-edit / Planner proposal (no new store). Amended PLAN_RETROSPECTIVE_NUDGE (graduate skills/scripts to machine mem + .agents/skills or scripts via reviewed lane). +5 prompt-wiring tests.
- heartbeat.rs: render_reflection wired after render_regression_gates + added to emptiness guard; reflect_every_gates() reads config.
- config/schema: [orchestration.reflect] every_gates default 15 (0 disables) + 2 tests.
- orchestrator_suffix.md: two pinned lines VERBATIM (scratch-worktree; deferred-evidence=OPEN RISK), non-scratch-edit=STOP tripwire, reflection-loop routing paragraph.
- planner_suffix.md: reflection symmetry line (skill promotion + task/test-plan quality as bounce cause).
- docs/CONFIG.md: reflect config + revision 25.

VERIFIED (static only — did NOT build, weak machine): pub-path refs, no exhaustive OrchestrationConfig literals, reflection.rs+reflection/tests.rs layout matches existing search_index.rs pattern, borrow/move correctness in pending_reflections, comments <=2 lines. Load was 4.57 but skipped cargo to avoid contending with the warm gate.

FOLLOWUP for orchestrator WARM GATE:
- CARGO_TARGET_DIR shared: scripts/test-capped.sh --lib -- service::plan::reflection  (9 trigger/render matrix tests)
- scripts/test-capped.sh --lib -- prompts   (reflection_block + suffix/retrospective wiring)
- scripts/test-capped.sh --lib -- config::schema   (reflect defaults/override)
- cargo check + fmt + clippy (both crates) at integration gate.
- Live dev round-trip acceptance: group close -> heartbeat Reflection block -> codeterm orchestrator learn -> learning visible + reflection cleared next heartbeat.

FLAG (system friction): bounces are not a first-class plan event (recorded as generic decision strides), so the window bounces count is an honest 0 until a bounce milestone kind exists — a follow-up task if we want real bounce cadence.
