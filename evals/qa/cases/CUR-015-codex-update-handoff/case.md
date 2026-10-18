# CUR-015 — Codex update and runtime handoff

## Purpose

Prove an installed binary update, visible updater output, runtime migration, and exact-session input continuity.

## Covers

Plan task keys: Migrated from codex-update.md; no task key was recorded. This remains release-core because updater/runtime restart changes can invalidate active sessions.

Defects: Updater output without binary replacement, runtime left on an old build, session migration to the wrong tab, or replayed input.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web, native
- **Platforms:** linux, macos, windows
- **Providers:** codex
- **Grants:** owner-update-grant, owned-codex-session, dev-build-install
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Pin installed build, boot, active Codex tab/session, and one unambiguous pending or idle state.
2. With an explicit owner update grant, apply the real update and record updater output and installed binary SHA.
3. Require the new runtime/build to serve the app and retain the exact owned session or the declared migration result.
4. Send one unique post-update marker and verify one own-session receipt; never use DEV source state as installed-binary proof.

## Oracle

PASS — Installed binary identity changes as expected, the runtime serves the new build, and the exact session accepts one post-update marker.

FAIL — The updater claims completion without the new binary/runtime, the session is misbound, or input is duplicated/lost.

NOT RUN — No explicit owner update grant, installed package, or disposable session is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
