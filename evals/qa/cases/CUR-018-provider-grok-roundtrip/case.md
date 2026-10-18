# CUR-018 — Grok PTY provider round-trip

## Purpose

Prove a fresh Grok PTY session completes one exact own-session round-trip.

## Covers

Plan task keys: The comprehensive-ready-lifecycle provider sweep records a prior accepted Grok PTY round-trip; the rebuilt 820 report does not claim a new Grok round-trip.

Defects: Fresh launch/readiness or own-session answer is missing or attributed to a restored UI row alone.
## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** linux, macos, windows
- **Providers:** grok
- **Grants:** provider-model-budget, owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Create one fresh owned Grok PTY fixture and verify its exact session becomes ready.
2. Send one unique marker once and require the exact own-session answer and idle state.
3. Record restore readiness as a separate observation; do not infer a round-trip from a sidebar status.

## Oracle

PASS — The fresh Grok session answers the unique marker once and becomes idle.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — A fresh owned Grok provider fixture or model budget is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — accepted fresh Grok round-trip on build 58a4167; restore readiness remains separately tracked in CUR-028 (target-check/evals/qa/2026-10-18-58a4167f/CUR-018/source-evidence.md).
