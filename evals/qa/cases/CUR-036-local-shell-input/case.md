# CUR-036 — Local shell input reaches the owned terminal

## Purpose

Prove a unique local shell command reaches one owned terminal process once and returns its expected output and exit status.

## Covers

Plan task keys: The rebuilt 820 report records taskless local shell input PASS: exact token once, same PTY, idle, exit 0.

Defects: Input is duplicated, sent to a foreign PTY, output is stale, or shell success is inferred from a UI echo.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, native
- **Platforms:** windows
- **Providers:** none
- **Grants:** owned-local-shell
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Create one disposable local shell with a unique command and record its PTY identity.
2. Submit once and require one exact output marker and process exit 0 from the same PTY.
3. Close only the owned shell after evidence is saved.

## Oracle

PASS — The exact command token appears once from the owned PTY and exits 0.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-820f8521/CUR-036/source-evidence.md.
