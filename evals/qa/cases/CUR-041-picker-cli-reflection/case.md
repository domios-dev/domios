# CUR-041 — Picker and CLI changes reflect bidirectionally

## Purpose

Prove a model/effort/speed change made through either picker or CLI appears through the other surface for the same session.

## Covers

Plan task keys: Owner-mandated post-rebuild reflection cell; no task key or execution was supplied.

Defects: Picker and CLI expose stale or divergent settings or reflection crosses provider/session ownership.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli, native, web, phone
- **Platforms:** windows
- **Providers:** claude, codex
- **Grants:** owned-provider-fixtures, model-change-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Set a model/effort value through the accessible picker and read it through CLI against the same tab/session.
2. Change a second value through CLI and require the picker to reflect it without changing session identity.
3. Repeat at desktop and 390x844 as the granted surfaces allow, then reload and verify persistence.

## Oracle

PASS — Both directions show the same resolved values for the same session before and after reload.

FAIL — Either direction diverges, updates a different session, or loses state after reload.

NOT RUN — The required picker and agent CLI handoff, identity, or fixture is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Owner-mandated cell remains NOT RUN until executed.
