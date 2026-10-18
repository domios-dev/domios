# CUR-011 — Restart, restore, and session continuity

## Purpose

Prove owner-controlled restart preserves task/session identity and restored tabs do not thaw or replay without an explicit visit/action.

## Covers

Plan task keys: Migrated from restart-restore.md; no task key was recorded for its legacy source journey.

Defects: Lost task binding, stale session identity, accidental thaw, replayed input, or claiming app restart from a frontend reload.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web, native
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** owner-restart-grant, owned-worker-fixture, owned-tabs
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Record an assigned task's task/attempt/tab/session identity and one retained idle tab.
2. Under an explicit owner restart grant, restart the named DEV instance and pin the new boot.
3. Verify task/session binding, restored idle state, and no replay before explicit selection/input.
4. Test frontend reload separately from full app restart; do not kill production.

## Oracle

PASS — The owned task and session remain bound after the granted restart, retained idle state is stable, and no input replays.

FAIL — Task/session identity changes unexpectedly, an idle tab thaws without action, or an input replays.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
