---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5377d92765a34ce7942f325a8cc81198
handle: plan-goal-north-star-ms_5377d92765a34ce7942f325a8cc81198
title: status_update
---

STATUS: done
CHANGES: none
BUILD/TYPECHECK/TESTS: not run; design-only brainstorm, no code changes
FOLLOWUP: §34 design essence recorded as decision stride ms_69609923cedc436db07965705f11b68b. Recommendation: GOAL is first-class Plan.goal metadata, not a task/section. Shape: {vision, vector, key_constraints[], postulates[], verify_checklist[], source_refs[], updated_at}. It is the north-star and handoff checksum, never decomposed or assigned.

Canvas: render GOAL as a pinned viewport banner plus a small plan-world anchor/coin. It stays readable at every zoom, composes with worlds W0-W3, and obeys §31: updates never move camera; off-screen changes only signal. Alternative A fixed viewport banner, B plan-node header only, C separate world node. Pick A+B: viewport banner for persistence, small canvas anchor for provenance.

Reinjection: every plan get/show includes goal; add codeterm plan goal set/show/update; heartbeat/delayed self-send includes compact goal vector + active postulates; worker dispatch and handoff reseed include it. Alternative CLI-only vs daemon-injected vs prompt-only. Pick daemon-injected plus CLI show because it survives context loss.

Postulates: store canonical list on Plan.goal and mirror procedure in .agents/skills/codeterm-orchestrator. Axioms: delayed heartbeat, re-read plan usage, mandatory review, real visual/TestFlight/Playwright verification, group-and-drive, periodic plan audit, single build gate, provider balance. Treat as reminders, not tasks.

Handoff verify-loop: compose handoff -> show/read back -> verify against main spec and GOAL three passes -> revise until complete -> only then compact/reseed if needed. Checks: goal/vector preserved, current state accurate, in-flight panes/branches/sessions listed, next actions executable, postulates/rules/gates present, open risks explicit. Safety: scratch/review mode first; live orchestrator context is touched only after verified. Fold this into codeterm-orchestrator final handoff section.

Concrete gaps: plan goal CLI; goal field on plan get/list JSON; canvas pinned renderer; heartbeat goal injection; handoff verify command, likely codeterm plan handoff verify --against-goal --passes 3; skill update; audit/reminder hook; goal/postulate provenance in plan record. Alternatives for verify command: integrated handoff create --verify, separate verify, or provider self-check prompt only. Pick separate verify plus create --verify sugar so the loop is explicit and testable.
