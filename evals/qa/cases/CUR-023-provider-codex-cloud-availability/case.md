# CUR-023 — Codex Cloud provider availability

## Purpose

Check that the separate Codex Cloud capability exposes an owned environment and completes a scoped round-trip.

## Covers

Plan task keys: The 820 provider inventory reports Codex Cloud environments=[]; no task key was recorded for a new cloud round-trip.

Defects: Cloud capability is inferred from local Codex success or an empty environment list is reported as a passed provider test.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** windows
- **Providers:** codex-cloud
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

PASS — An owned Cloud environment is available and returns one exact own-session result.

FAIL — An available environment fails its documented own-session request.

NOT RUN — No Cloud environments are available; the case remains blocked without fabricating one.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — BLOCKED — target-check/evals/qa/2026-10-18-820f8521/CUR-023/source-evidence.md (provider inventory returned environments=[]).
