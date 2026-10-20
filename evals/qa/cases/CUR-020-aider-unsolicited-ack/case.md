# CUR-020 — Aider unsolicited acknowledgement after routed work

## Purpose

Check that a provider does not emit unrequested follow-up turns after a completed routed exchange.

## Covers

Plan task keys: task_cjymmf — the 820 report separates a subsequent unsolicited acknowledgement loop from the passing one-turn round-trip.

Defects: Unsolicited acknowledgements are attributed to a user request or continue after the one intended response.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** linux, macos, windows
- **Providers:** aider
- **Grants:** owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Complete a one-turn owned exchange, then observe the same session without sending additional content.
2. Record any later provider output with its source ID and timing; do not provoke it with another send.
3. Stop observation under the bounded owner deadline and preserve exact evidence.

## Oracle

PASS — The provider emits no unrequested additional acknowledgement after the expected reply.

FAIL — The provider emits an unsolicited follow-up/acknowledgement loop in the own session.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — task_cjymmf; target-check/evals/qa/2026-10-18-820f8521/CUR-020/source-evidence.md.
