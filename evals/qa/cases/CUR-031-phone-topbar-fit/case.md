# CUR-031 — Phone top bar fits the viewport

## Purpose

Verify all required top-bar actions remain inside a 390px-wide phone viewport.

## Covers

Plan task keys: The 820 rebuilt phone report records the Update button extending beyond the 390px viewport; no task key was attached to that observation.

Defects: A visible top-bar control lies outside the responsive viewport or cannot be reached.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** phone
- **Platforms:** windows
- **Providers:** none
- **Grants:** none
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Open the owned web mirror at 390x844 and record viewport and visible top-bar control bounds.
2. Verify the Update action and other required controls remain within the viewport and can be activated.

## Oracle

PASS — Every required top-bar action is inside the viewport and reachable.

FAIL — A required control extends beyond the viewport or cannot be activated.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — rebuilt-phone-report.md; target-check/evals/qa/2026-10-18-820f8521/CUR-031/source-evidence.md (Update button right edge 425.5px on a 390px viewport).
