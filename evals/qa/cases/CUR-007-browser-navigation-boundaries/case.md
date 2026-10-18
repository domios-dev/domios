# CUR-007 — Browser stale references and page boundaries

## Purpose

Exercise stale element references, held navigation, bounded waits, iframe scope, and popup target ownership.

## Covers

Plan task keys: Migrated controlled-boundary steps from browser.md; no task key was recorded.

Defects: A stale reference acts on a different page, a held navigation is called complete without settling, or an untracked frame/popup is presented as covered.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

regression

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-browser, loopback-fixture-server
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Capture a reference, take a new snapshot, then require the old reference to refuse; repeat after SPA navigation.
2. Use the loopback held-navigation fixture and record whether the action returns before release; then explicitly release and wait on destination text.
3. Inspect iframe and popup through the tracked Browser target and report unsupported boundaries without switching automation surfaces.

## Oracle

PASS — Stale references refuse safely, waits use actual destination events, and frame/popup support is represented accurately.

FAIL — Stale references act on a new target, a held page is claimed ready, or a popup/frame is claimed covered without own-target proof.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
