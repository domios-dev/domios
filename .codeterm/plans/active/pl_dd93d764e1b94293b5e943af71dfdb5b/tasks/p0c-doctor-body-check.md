---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_764662977b64477fb47611eb54344321
handle: p0c-doctor-body-check
title: 'P0c: quality doctor must read externalized task fields'
status: done
---

Fix the quality doctor's externalized-body blindness so the doctor gate is trustworthy BEFORE UX lane dispatch. Extracted from t0-planning-kind (criterion 3) into its own gating task after the false positives blocked orchestration twice on this plan.

Reproduced evidence (this plan, twice): every task body is externalized by the body_files layer to .codeterm/plans/active/<plan>/tasks/<handle>.md; the inline plan.json body field stays empty by design; the quality doctor reads ONLY the inline field -> `task is missing body` for all 18 tasks while `plan get --json` (daemon API) returns full bodies (1078-2524 chars each).

Steps:
1. Quality doctor (task_quality_gap rule in src-tauri/src/service/plan hints/next_actions area): resolve task bodies through the same body_files read path the API uses — a task with a non-empty sidecar body is NOT missing a body. Audit the other quality-checked fields (intent, role_guidance, criteria, test_plan, correctness) for the same inline-vs-externalized mismatch and fix any that read raw plan.json.
2. Regression tests: task with externalized body passes; task with genuinely no body (no inline, no sidecar) still errors; mixed plans report only the real gaps.
3. Verify against THIS plan: `codeterm plan doctor pl_dd93d764e1b94293b5e943af71dfdb5b --kind quality` must report 0 task_quality_gap errors, since all 18 tasks have real bodies.

Commands: narrowed cargo test (service::plan), single run; ./build.sh check; ./build.sh cli; live doctor run against this plan quoted in the report.
Report: which fields were inline-blind, test output, live doctor before/after counts on this plan.
