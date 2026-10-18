# CUR-030 — Phone composer controls stay in the action row

## Purpose

Check attachment and Stop controls in a 390x844 phone viewport while the composer is idle and busy.

## Covers

Plan task keys: task_kkvrw4 — rebuilt report: idle attachment is reachable; during a busy turn Stop was 56px above the attachment action row.

Defects: A required action moves outside the expected action row or leaves the viewport while the composer is busy.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** phone
- **Platforms:** windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. At 390x844, record the idle composer and attachment button bounds.
2. Start an owned bounded turn, wait for working, and record Stop/attachment/composer geometry.
3. Compare action row alignment without using CSS class names as the behavior oracle.

## Oracle

PASS — Required idle and busy controls remain reachable and aligned in the declared phone layout.

FAIL — Busy Stop is separated from the attachment action row or a required control is clipped.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — task_kkvrw4; target-check/evals/qa/2026-10-18-820f8521/CUR-030/source-evidence.md (Stop button bounds were 56px above attachment actions at 390x844).
