# CUR-022 — Cursor provider availability

## Purpose

Keep the Cursor provider round-trip as an explicit selectable release row.

## Covers

Plan task keys: No live task key; owner decision defers this provider while it is not installed in the QA environment.

Defects: An absent provider is silently counted as covered or reported as a product failure.
## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** windows
- **Providers:** cursor
- **Grants:** none
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Use only an owned DEV caller and the declared surface.
2. Perform each action once and wait for the exact far-end state.
3. Record the returned IDs and compare the visible state with the own-session or persisted record.

## Oracle

PASS — When installed and granted, Cursor reaches ready and completes an exact own-session round-trip.

FAIL — An installed, granted Cursor provider cannot launch or return its own-session answer.

NOT RUN — Cursor is absent and explicitly owner-deferred for this release.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. See the OWNER-SKIPPED decision in matrix/1.14.9.md.
