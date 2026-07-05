---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_81b9cd0b7b4d48eabb29e6cbb94d657c
handle: orchestrator-creation-test-matrix-every-route-executable-scenarios-ms_81b9cd0b7b4d48eabb29e6cbb94d657c
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/orchestrator-creation-test-matrix (pushed, sha 0024dc9663f04a8e41f106105defa5c5e33e6c84)
CHANGES: new eval set evals/orchestrator-creation-matrix/ — run.sh (559L, dc-wrapped/dev-locked runnable spec for routes B-H), README.md (route map + verdict vocab + gate wiring), RESULTS.md (live transcript + findings). No product code touched (QA lane).

BUILD/TYPECHECK/TESTS: bash -n clean. Ran the zero/low-agent scenarios LIVE vs dev (builds ea2e30e9->905e2d00; machine load spiked 15-23 mid-run as you were building other lanes).
  LIVE PASS: F.note_required ('--planner none' without --note refused), F.waiver ('--planner none --note' => planner.kind=waived), F.nudge_state (waived => nudge prefix None).
  LIVE FINDING G: 'plan doctor --kind quality' has NO planner_unpaired rule at any state — emits only missing_goal/missing_spec_source. The pinned WARNING->ERROR->silent planner ladder is NOT wired in dev. Bounce -> doctor/QA-enforcement lane task_084e2148.
  DEFERRED (load>10, spawn real claude panes): B (orchestration new pair), C (role spawn), D (make-orchestrator RPC), E (planner bind), H1/H2/H3 negatives. Harness is written+syntax-valid; run 'bash evals/orchestrator-creation-matrix/run.sh' in a low-load window.

FOLLOWUP / bounces for you to route:
  1. G: planner_unpaired doctor ladder missing -> task_084e2148.
  2. C/D: onboarding ships literal '--plan PLAN_ID' placeholder (not a real plan id) — pinned C wants real id -> make-orchestrator/pair lane task_aa01c0db.
  3. D: no 'codeterm orchestrator promote' CLI; only POST /api/panes/{id}/make-orchestrator (harness uses dev_post).
  4. H2: no BACKEND hierarchy guard on 'orchestration new'/'spawn --role Orchestrator' — worker 'never spawn' is prompt-only; promotion does reject parented panes -> hierarchy-guard lane.
  5. Infra bugs biting under load: 'database is locked' on plan create; 'plan_index_mirror is pending after committed write' (task_354c755f6e class) false-fail on plan update; CLI returns rc=0 on soft '✗' errors; dc no-ops (exit 7) when load>~15 (1s health gate). Harness retries around these.
  6. GATED by design: async nudge lifecycle (nudge_interval_secs=1800) not observable in bounded run; asserted via deterministic planner-binding driving state (Unbound=>fires, Planner/Waived=>stops).
  7. Wire matrix into FINAL acceptance task_30b13c9be185472f982b0c26a6182235 as a required green gate (see README).
  8. Route I (in-place picker UI) intentionally left to Playwright picker spec task_234c14f04d9f4dbfb5a743d50486617d.
