# CUR-038 — Teams task lifecycle and provider-bound worker recovery

## Purpose

Check a Team-bound task through loss recovery, provider switch, owner correction, report, and cleanup with one live attempt.

## Covers

Plan task keys: TeamQA Plan pl_p9swxl/task_6u422c Round 2 report: duplicate respawn created two attempts; in-place provider switch retained the prior task session and repeated ACK was refused; final value intentionally not run.

Defects: Duplicate live attempts, task/session binding mismatch, false ACK/report success, or dirty fixture loss during cleanup.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** team, cli
- **Platforms:** windows
- **Providers:** none
- **Grants:** disposable-team-plan, owned-orchestrator-worker, explicit-task-delegation
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Use a disposable Team, Plan, and task. Record task attempt and worker session before recovery.
2. Exercise exactly one lost-worker recovery and verify one live attempt; switch provider only under a declared grant and verify task/session binding.
3. Do not repeat refused transitions. Verify far-end report only on a current attempt and preserve dirty lane evidence until its owner resolves it.

## Oracle

PASS — One attempt remains bound to the current worker session and the final approved value reaches the parent once.

FAIL — Duplicate attempts appear, provider switch/ACK binding mismatches, or the report is accepted against stale state.

NOT RUN — The owner has not supplied a corrected disposable fixture or delegated a safe repeat.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — TeamQA d1bd77da/round2-report.md; target-check/evals/qa/2026-10-18-820f8521/CUR-038/source-evidence.md (duplicate respawn and provider-switch binding defect; final value not run).
