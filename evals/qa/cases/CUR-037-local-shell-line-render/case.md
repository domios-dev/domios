# CUR-037 — Local shell output does not overprint prior rows

## Purpose

Check long colored local shell output preserves row order and the final marker in the visible terminal.

## Covers

Plan task keys: The rebuilt 820 report records a visual overprint candidate in earlier rows; exact owned local shell was closed and not reused.

Defects: Rendered lines overwrite prior output or the final marker is clipped while the process output is correct.
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

1. Use a fresh owned shell fixture to print uniquely numbered colored rows and a final marker.
2. Compare the opened terminal screenshot with exact process output and scrollback row order.
3. Do not repeat a closed fixture by guessed ID; create a new authorized fixture for a new run.

## Oracle

PASS — Every visible row appears once in the correct order and the final marker is fully visible.

FAIL — Rows visually overprint or the terminal rendering disagrees with the same process output.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — rebuilt-local-shell-report.md; target-check/evals/qa/2026-10-18-820f8521/CUR-037/source-evidence.md.
