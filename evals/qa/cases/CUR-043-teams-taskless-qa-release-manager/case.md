# CUR-043 — Task-less QA and release-manager worker kinds

## Purpose

Charter: verify task-less QA and release-manager placement preserves worker kind and report authority.

## Covers

Plan task keys: task_fn0aoi — accepted canonical QA/release-manager placement requirements; the requested Round 3 live charter remains NOT RUN.

Defects: Task-less workers gain an unintended task binding, lose their assigned kind, or report through task-bound transition verbs.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** windows
- **Providers:** none
- **Grants:** owner-taskless-spawn-grant, disposable-team-plan
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: spawn task-less QA and release-manager roles and inspect their actual metadata, identity, and allowed reporting path.
2. Scope: disposable Team and owned task-less workers only; do not create source tasks or notify external recipients.
3. Time box: 30 minutes; stop on role/kind mismatch.
4. Heuristics: compare pre/post role, task binding, report reception, and refusal behavior; treat user-facing send success separately from parent receipt.
5. Regression follow-up: convert each fixed kind or authority defect into a focused case with a task key and persisted parent-state oracle.

## Oracle

PASS — Each role remains task-less with the intended kind and its report reaches the parent through the authorized path.

FAIL — A worker is silently task-bound, kind changes, or an unauthorized transition succeeds.

NOT RUN — No Round 3 live fixture or exact delegated authority is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Round 3 charter is NOT RUN.
