# CUR-003 — Responsive chat affordances

## Purpose

Check composer controls, Stop, attachment placement, and usable chat content at desktop and 390x844 responsive dimensions; keep real-device coverage explicit.

## Covers

Plan task keys: task_kkvrw4 for phone composer controls; real-iPhone device coverage is owner-deferred.

Defects: Off-row or clipped controls, inaccessible chat actions, or a phone emulation reported as a physical-device result.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** phone, web
- **Platforms:** linux, macos, windows, ios
- **Providers:** none
- **Grants:** dev-browser-auth, desktop-viewer, dev-fixture-spawn
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use an owned web chat at desktop size and a 390x844 viewport. Record whether the phone surface is browser emulation or a physical iPhone.

## Steps

1. Inspect composer and attachment controls at desktop and 390x844.
2. While idle, verify attachment actions remain reachable beside the composer.
3. While working, verify Stop and attachment controls remain in the expected action row and within the viewport.
4. Record physical-device checks separately from browser emulation.

## Oracle

PASS — Required chat controls remain visible, reachable, and aligned for each selected target; device type is recorded accurately.

FAIL — A required control is clipped, misaligned, or unavailable, or emulation is claimed as physical-device proof.

NOT RUN — The browser, provider, fixture, viewport, or physical device required by the selected row is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — rebuilt-phone-stop-report.md and comprehensive-wave/mobile-controls-permission.json (responsive controls inspected; physical iPhone was unavailable).
