---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_aecabe5aca874ae2b24a5ebb5b33beec
handle: plan-goal-north-star-ms_aecabe5aca874ae2b24a5ebb5b33beec
title: status_update
---

DONE: STATUS: done (DESIGN ONLY, no code)
CHANGES: none (design); 6 decision strides recorded on task_6e00afe9

ESSENCE — §34 GOAL + postulates + handoff-verify-loop:
1. GOAL MODEL: first-class Plan.goal: Option<PlanGoal>{vector, vision, constraints[], postulates[], updated_at/by} in meta.yaml. North-star, NOT a section/task: no work, no coverage, never done, singleton. Confirmed MISSING today (Plan struct types.rs:265 has no goal; only handoff.rs has transient goal:Option<String>). Rejected: model as Spec (re-merges §29 split) / free-text title.
2. CANVAS: screen-space PINNED ribbon (HUD chrome outside pan/zoom), collapsed=vector line / expandable. Camera-immune → always-visible + readable at every W-band; composes §31 (no camera move) + §35 (plan-sun stays largest NODE, goal is a separate HUD plane). Rejected world-space banner (unreadable at W0, moves w/ camera).
3. REINJECTION: 3 surfaces — goal block at TOP of plan get; plan goal show [--postulates-only] on-demand; goal+enabled postulates inlined in the delayed HEARTBEAT self-send (the periodic engine).
4. POSTULATES: 8 canonical (self-heartbeat, re-read plan usage, always-review, real visual testing, group-and-drive, periodic plan audit, single build-gate, provider-balance). Two homes: terse axioms in Plan.goal.postulates[] (ride reinjection) + detailed prose in codeterm-orchestrator SKILL.md. Default-seeded on goal set.
5. VERIFY-LOOP (extends §27): compose(handoff --verify)→show read-back→self-verify vs MAIN SPEC ×3→only then compact/reseed→fold into SKILL.md on live proof. 5 spec-completeness checks/pass: GOAL/STATE/IN-FLIGHT/NEXT-ACTIONS/RULES. New handoff.verify_state{composed→verifying→verified→reseeded} GATES the /new reseed (daemon refuses unless verified||--force). Verify in-place in live orchestrator (owns context); --scratch dry-run optional.
6. GAPS: Plan.goal types + meta.yaml; handoff.verify_state/verify_passes; CLI plan goal set/show/postulates + set_goal batch op + orchestrator handoff --verify; goal block in plan get; heartbeat template reinjection; canvas ribbon; SKILL.md GOAL section + verify-loop + fold-on-live-proof; verify_state reseed gate in §27.3.

BUILD/TYPECHECK/TESTS: not run (design only)
FOLLOWUP: Plan.goal slots cleanly into §33 meta.yaml (slim scalar/prose, not work arrays). verify_state field extends §27.3 reseed monotonic states — coordinate with whoever builds handoff. Canvas ribbon is NEW chrome, independent of worlds-v3 node layout.
