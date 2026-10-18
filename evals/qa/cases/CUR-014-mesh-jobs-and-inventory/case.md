# CUR-014 — Mesh inventory and persisted job results

## Purpose

Verify manual and scheduled work leave exact persisted results and a read-only mesh view reflects the granted peer inventory.

## Covers

Plan task keys: Migrated from mesh-and-jobs.md; no exact task key was captured for the legacy journey.

Defects: Manual success used to infer scheduled execution, stale/fictional peer inventory, scope leakage, or results detached from the originating run.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, mesh, team
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** owned-job-fixture, mesh-read-only-peer-grant, scheduled-job-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Create a bounded manual job and one future one-shot scheduled job in the owned workspace.
2. Require the manual final value and scheduled event/persisted result independently; delete the scheduled job before it fires if still pending.
3. Read mesh peers without pairing or remote mutation and compare the rendered inventory with the granted peer state.

## Oracle

PASS — Each executed job has its own durable expected result and the read-only mesh inventory accurately reflects the authorized peers.

FAIL — Scheduled execution is inferred from manual success, results are missing/wrong, or peer scope is stale or leaked.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
