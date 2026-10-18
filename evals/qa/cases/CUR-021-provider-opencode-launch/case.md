# CUR-021 — OpenCode default provider launch

## Purpose

Prove the documented default OpenCode launch reaches ready state with an available model and one exact own-session response.

## Covers

Plan task keys: task_seo1tl — rebuilt 820 default launch failed because unspecified model resolved to missing mimo secret `domios/model-provider/mimo`.

Defects: Default launch uses an unavailable secret alias or reports readiness after launch failure.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** linux, macos, windows
- **Providers:** opencode
- **Grants:** owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Use one owned disposable caller and invoke the documented default OpenCode launch exactly once.
2. Record requested/resolved model, launch status, raw exit, and provider session.
3. Do not retry an identical launch after the receipt identifies the missing secret; close only the failed owned fixture.

## Oracle

PASS — The default launch reaches agent_ready and answers one unique marker in its own session.

FAIL — The default launch fails on an unresolved/unavailable model secret or falsely claims ready.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — task_seo1tl; target-check/evals/qa/2026-10-18-820f8521/CUR-021/source-evidence.md.
