# CUR-039 — Composer picker model and settings changes

## Purpose

Exercise exposed model, effort, speed, permission, sandbox, and preset controls through accessible composer-picker UI on native desktop, web desktop, and 390x844.

## Covers

Plan task keys: Owner-mandated post-rebuild cell; no task key or run was supplied. task_fn0aoi is cited only for canonical QA/release-manager placement, not as execution evidence.

Defects: Picker accepts a change without runtime state, changes the wrong provider/session, or inaccessible controls are bypassed with undocumented selectors.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** native, web, phone
- **Platforms:** windows
- **Providers:** claude, codex, grok, opencode
- **Grants:** owned-provider-fixtures, model-change-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use owned Codex chat_backend and Claude PTY fixtures, then available Grok/OpenCode only if ready. Pin runtime model, effort, serviceTier, and session identity.

## Steps

1. On each granted surface, inspect controls by accessible role/name and record available state.
2. Change one setting at a time, then verify resolved runtime model/effort/serviceTier and one unique next own-session answer.
3. Record reload persistence and session continuity or one declared relaunch; report missing accessible roles as BLOCKED-A11Y without selector workarounds.

## Oracle

PASS — Each supported picker change updates the declared runtime state and the next own-session answer uses that state on all selected surfaces.

FAIL — A setting appears changed without runtime reflection, affects another session, or is reachable only by undocumented selector workarounds.

NOT RUN — The required post-rebuild handoff/identity, provider fixture, accessible control, or grant is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Owner-mandated cell remains NOT RUN until executed.
