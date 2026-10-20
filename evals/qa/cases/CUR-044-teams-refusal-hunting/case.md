# CUR-044 — Teams and agent refusal hunting

## Purpose

Charter: probe boundary refusals for task attempts, provider switches, adoption, and reporting without mutating foreign state.

## Covers

Plan task keys: Owner-mandated Teams Round 3 charter; historical refusal defects in task_6u422c provide seed cases, but this charter has no run receipt.

Defects: Unsafe command succeeds, refusal is misleading, a failed action partially mutates state, or a refusal message hides the required recovery path.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** windows
- **Providers:** none
- **Grants:** disposable-team-plan, owner-refusal-hunt-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: hunt refusal boundaries across a disposable Team/Plan lifecycle.
2. Scope: own fixtures only; no real user tasks, production calls, or retries after uncertain writes.
3. Time box: 30 minutes; preserve the first surprising result.
4. Heuristics: try stale attempt, wrong provider, wrong task, missing delegation, busy switch, duplicate recovery, and task-less transition forms; compare before/after state after every refusal.
5. Regression follow-up: fixed defects become regression cases; unclassified product refusals get a narrow owner decision before retest.

## Oracle

PASS — Each unsafe or stale request refuses clearly and leaves owned task/attempt/session state unchanged.

FAIL — A forbidden transition succeeds, state mutates despite refusal, or the response falsely claims a successful operation.

NOT RUN — No disposable owner-granted Round 3 fixture is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Round 3 charter is NOT RUN.
