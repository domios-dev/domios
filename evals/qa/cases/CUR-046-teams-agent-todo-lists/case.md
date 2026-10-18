# CUR-046 — Teams agent todo lists

## Purpose

Charter: explore agent todo creation, progress, completion, and reporting across a disposable Team task.

## Covers

Plan task keys: Owner-mandated Teams Round 3 charter; no task key or execution was supplied.

Defects: Todo items detach from the task, show false completion, reorder incorrectly, or report to the wrong parent.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** team, web
- **Platforms:** windows
- **Providers:** none
- **Grants:** disposable-team-plan, owned-worker-fixtures
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: exercise todo add/update/complete and review behavior within one disposable Worker task.
2. Scope: one owner-granted task and its assigned Worker; no external notifications or real task transitions.
3. Time box: 30 minutes; preserve the first discrepancy and stop on stale attempt state.
4. Heuristics: compare todo IDs, order, state transitions, task attempt, and parent report before/after worker restart or provider change where explicitly allowed.
5. Regression follow-up: a fixed todo defect becomes a separate regression case keyed to the accepted task and persisted task/parent state.

## Oracle

PASS — Todo IDs and state remain associated with the current task attempt, and completion/report state agrees at the parent.

FAIL — Todo state is lost, misbound, falsely complete, or disagrees with the parent report.

NOT RUN — No owner-approved disposable Team and Worker task fixture is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Round 3 charter is NOT RUN.
