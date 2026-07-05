---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_725754415ca34fbeb508b55cf0c1b052
handle: planner-lane-ms_725754415ca34fbeb508b55cf0c1b052
title: status_update
---

PARTIAL: Doctor-gap follow-up: VERIFIED FALSE POSITIVE, not missing planner content — no task bodies were rewritten because none are missing.
EVIDENCE (fresh, this session): plan get --json (daemon API) shows full bodies on ALL tasks (1078-2524 chars) plus criteria/test-plan/correctness/role_guidance/section links; all 18+ tasks/<handle>.md sidecar files exist with the content; plan.json inline body is empty on all 18 BY DESIGN (body_files externalizes every body); the quality doctor reads ONLY the inline field. Workers DO have self-contained briefs — task show / plan get / the sidecars all carry them; only the doctor is blind.
ACTIONS TAKEN: (1) NEW gating task p0c-doctor-body-check (task_764662977b64477fb47611eb54344321, p1, sonnet, wave-p0): doctor must read task fields via the body_files read path; hard oracle = doctor on THIS plan drops 18->0 task_quality_gap without any task edits; genuinely-missing-body still errors. (2) t0-planning-kind rescoped (doctor-body criterion moved out to p0c; t0 is pending/unassigned so safe). (3) Planner-process rule added to the planning-ownership SPEC section (dispatch requires clean quality doctor; a false-positive doctor is itself gating work) — flows to the active t0c docs worker through its linked spec section; no in_progress task fields were touched (p1b, t0c untouched per your instruction). (4) Stride recorded.
DOCTOR NOW: 19 errors (18 known false positives + new p0c task itself, same false positive). Expected red until p0c lands.
NEXT: dispatch p0c-doctor-body-check to any free rust worker (small, sonnet, fast); treat doctor as gating signal only AFTER p0c's oracle passes; UX lane dispatch discipline then becomes: doctor clean -> dispatch.
