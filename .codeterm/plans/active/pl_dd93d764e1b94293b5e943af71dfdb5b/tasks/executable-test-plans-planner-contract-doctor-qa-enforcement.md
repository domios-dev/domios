---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_084e2148faef4c4d93a1cb39d84475a0
handle: executable-test-plans-planner-contract-doctor-qa-enforcement
title: 'Executable test plans: planner contract + doctor/QA enforcement'
status: done
---

## What

User-observed failure mode (round-15): task test plans are written as intentions ("component tests; manual check"), workers fill the gaps with guesses, and non-working solutions pass gates. Make executable test plans a CONTRACT: prompt, docs, doctor, and QA-queue enforcement.

## EXACT PROMPT TEXT (pinned — implement verbatim, extends the oracle-first paragraph from task_70c4a935dd6a4abf87a679915a3636b5 in planner_suffix.md; same shared-constant module)

```
Test plans are EXECUTABLE, not descriptive. A test plan names: (1) the exact command(s) to run, (2) the exact surfaces touched (testids, selectors, files, endpoints), (3) numbered steps with the observable pass condition for each. If the executing worker would have to invent a selector, a step, or an expected value, the plan is incomplete — write the scenario, not the intention. "Component tests + manual check" is not a test plan.
```

## Enforcement (mechanical, not vibes)

1. **Doctor (quality)**: warning `test_plan_vague` when a non-planning task's test_plan is under ~120 chars OR contains no command-like token (backtick, `npx`, `cargo`, `codeterm`, `bash`, `curl`). Threshold structural, not prose-semantic — consistent with doctor philosophy; tune constants in one place.
2. **QA queue**: `plan qa next --claim` already prints the task's test_plan; when the vague-predicate fires, the claim output prepends: "TEST PLAN IS VAGUE — bounce to the orchestrator for a concrete plan before verifying." Tester charter constant gains one line: a vague test plan is itself a bounce reason.
3. **Docs**: ORCHESTRATION doc test-plan subsection gets the format contract + one GOOD example (the 10-scenario Playwright spec from the picker-v2 task body is the reference example — link it).

## Reference standard

The picker-v2 task (task_234c14f04d9f4dbfb5a743d50486617d) body carries the model example: numbered scenarios, exact testids, exact pass conditions, cleanup rules, exact run command. That is the bar.
