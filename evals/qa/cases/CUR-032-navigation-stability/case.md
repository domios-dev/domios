# CUR-032 — Owned web navigation stays bounded

## Purpose

Check that expected open/reload navigation occurs and no unsolicited navigation appears during the declared observation window.

## Covers

Plan task keys: task_ty5ff6 — owner accepted the bounded 820 navigation observation: three expected root navigations and zero unsolicited navigation during 600 seconds.

Defects: Unsolicited navigation, missed intended reload, or a timeout misreported as a clean cold-install result.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-browser
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Pin the owned Browser target, DEV build, and expected URL.
2. Record the initial open and two explicit reloads with event timestamps.
3. Observe the declared window for unsolicited navigation; report the observer deadline and do not claim cold-install coverage.

## Oracle

PASS — Exactly the expected initial open and two explicit reloads occur, with zero unsolicited navigation in the bounded 600-second observation.

FAIL — An unsolicited navigation occurs or an expected navigation is missing/duplicated.

NOT RUN — The owned Browser identity or complete bounded observation record is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_ty5ff6; target-check/evals/qa/2026-10-18-820f8521/CUR-032/source-evidence.md (accepted bounded scope; not a cold-install claim).
