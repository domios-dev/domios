# CUR-029 — Draft survives tab switch and reload

## Purpose

Prove unsent draft text remains attached to its own tab after a tab switch and page reload, without becoming a send.

## Covers

Plan task keys: The 820 wave recorded same draft persistence through an owned tab roundtrip and reload, cleared without sending.

Defects: Draft loss, draft crossing tabs, or draft text submitted during reload/cleanup.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, phone
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Enter a unique draft without submitting it and record the owning tab.
2. Switch away and back, then reload the view; require the same text in the same tab's composer.
3. Clear through keyboard input and require zero user-turn IDs for the draft marker.

## Oracle

PASS — The draft remains in its source tab through both transitions and is cleared without a send.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-820f8521/CUR-029/source-evidence.md.
