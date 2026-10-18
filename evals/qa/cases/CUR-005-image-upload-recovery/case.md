# CUR-005 — Image upload pending and failure recovery

## Purpose

Check pending upload status and bounded recovery without duplicating the prompt or attachment.

## Covers

Plan task keys: Migrated image pending/failure subcase; no plan task key was recorded.

Defects: A failed upload appears sent, pending state is lost, or a retry duplicates a previously written input.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, phone
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-browser-auth, image-fixture, controlled-upload-failure
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Exercise one normal pending-to-complete upload and one explicitly granted controlled failure.
2. Record attachment IDs and composer state at each transition.
3. Recover only after exact non-delivery is proven; preserve uncertain writes and do not replay.

## Oracle

PASS — Pending and failure states are visible and honest; recovery sends at most one confirmed-undelivered input.

FAIL — A failed or uncertain upload is reported delivered or recovery duplicates an attachment or message.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
