# CUR-034 — Remote shell lines reflow on phone

## Purpose

Check long remote shell rows reflow to the phone terminal column width instead of remaining desktop-width and clipping.

## Covers

Plan task keys: task_8yukt0 — rebuilt report: remote shell retained 163-cell lines in a 45-column 390x844 viewport and clipped instead of reflowing.

Defects: Remote shell output clips, overprints, or loses its final marker when the phone terminal is narrower than the source row.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** mesh, phone
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

1. On the owned remote shell, print a unique line longer than the 390x844 viewport width.
2. Inspect remote buffer dimensions, wrapped rows, full token, and scrollback before/after a wheel event.
3. Keep remote input unrun unless separately granted.

## Oracle

PASS — Long output is reflowed to the visible phone width and its full text remains recoverable.

FAIL — The remote buffer retains a 163-cell line in a 45-column view and clips rather than reflowing.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — task_8yukt0; target-check/evals/qa/2026-10-18-820f8521/CUR-034/source-evidence.md (163-cell line in 45-column view).
