# CUR-042 — Teams reconfiguration and adopt-plan

## Purpose

Charter: test Team reconfiguration and Plan adoption while preserving task and worker authority.

## Covers

Plan task keys: Owner-mandated Teams Round 3 charter; task key for this new session was not supplied. Historical adoption/lifecycle evidence is TeamQA task_6u422c.

Defects: Configuration changes silently transfer authority, lose a Plan/task binding, or leave orphaned workers/lanes.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** windows
- **Providers:** none
- **Grants:** disposable-team-plan, owner-reconfiguration-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: reconfigure an owned disposable Team and adopt a Plan without changing unrelated owner state.
2. Scope: one disposable Team/Plan and bounded task fixtures; no real tasks or lane deletion.
3. Time box: 45 minutes; stop at the first unexplained binding or cleanup mismatch.
4. Heuristics: change one Team role/config field at a time; inspect before/after membership, Plan adoption, task attempt/session binding, and cleanup ownership.
5. Regression follow-up: turn each reproducible fixed defect into a release-core/regression CUR case with exact task key and far-end oracle.

## Oracle

PASS — Team configuration and adopted Plan remain correctly bound, and all owned resources are accounted for.

FAIL — Authority or task binding changes unexpectedly, an unowned task is touched, or cleanup loses an owned fixture.

NOT RUN — No disposable owner-approved Round 3 Team/Plan fixture is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Round 3 charter is NOT RUN.
