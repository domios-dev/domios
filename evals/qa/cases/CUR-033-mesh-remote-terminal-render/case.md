# CUR-033 — Mesh remote terminal renders and scrolls

## Purpose

Prove remote shell and Claude Terminal output render in the local view and owned scroll changes remote scrollback at desktop and responsive dimensions.

## Covers

Plan task keys: task_8yukt0 — parent accepted remote terminal render coverage on rebuilt DEV, including desktop/390x844 paint and wheel-driven scrollback.

Defects: Remote output is absent/stale, local and remote terminal ownership is confused, or scroll does not move the owned buffer.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** mesh, phone, web
- **Platforms:** windows
- **Providers:** none
- **Grants:** mesh-online-peer-read, owned-remote-terminal
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Use the granted online mesh peer and record the remote tab/session identity.
2. Open the owned remote shell and Claude Terminal at desktop, then at 390x844.
3. Compare a unique output token and exact remote buffer state; scroll once and prove the remote viewport changes.
4. Separate the shell phone line-wrap defect into CUR-034.

## Oracle

PASS — Owned remote terminal output is visible at desktop and 390x844 and scroll input changes its remote buffer.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_8yukt0; target-check/evals/qa/2026-10-18-820f8521/CUR-033/source-evidence.md.
