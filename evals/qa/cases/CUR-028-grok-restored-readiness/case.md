# CUR-028 — Grok restored provider readiness

## Purpose

Verify a restored Grok tab finishes loading and returns to an exact own-session idle state.

## Covers

Plan task keys: The 820 rebuilt report records task_59535k blocked: Grok restore remained Restoring/pty_starting and no completed-load/reload oracle was available.

Defects: Sidebar idle is mistaken for loaded provider state or a restored fixture is force-restarted to manufacture readiness.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, cli
- **Platforms:** linux, macos, windows
- **Providers:** grok
- **Grants:** owned-provider-fixture
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

PASS — The same Grok session completes load and returns an exact own-session response.

FAIL — A fully loaded restored Grok session changes identity or loses the expected turn.

NOT RUN — The restore remains in Restoring/pty_starting; no retry or input is authorized to force readiness.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — BLOCKED — task_59535k; target-check/evals/qa/2026-10-18-820f8521/CUR-028/source-evidence.md.
