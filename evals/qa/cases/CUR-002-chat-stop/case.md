# CUR-002 — Stop interrupts and stays stopped

## Purpose

Prove Stop interrupts a real working turn and the same session remains idle and interrupted after the view reloads, without replay.

## Covers

Plan task keys: Comprehensive-wave API Stop and phone UI Stop receipts; no dedicated plan task key was recorded for these Stop cells.

Defects: False Stop success, a continuing turn, changed session, replay, or a child process mistaken for parent completion.
## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web, phone
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-fixture-spawn, owned-long-turn, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use an owned tab with a bounded long turn that emits a real working event. Record the turn, session, and message count.

## Steps

1. Start one turn and wait for the actual working event.
2. Issue the supported Stop action once and require the same session to become idle with interrupted status.
3. Reload the view and require the same turn to remain interrupted and the user input to appear once.
4. Compare parent and child activity separately; stop an owned child only with its separate grant.

## Oracle

PASS — The observed working turn stops, the same session becomes idle, and the interrupted turn remains once after reload.

FAIL — Stop reports success without interruption, the session changes, the input replays, or parent completion is inferred from a child process.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-820f8521/CUR-002/source-evidence.md (Codex backend Stop and 390x844 phone UI Stop reached same-session idle/interrupted state).
- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-58a4167f/CUR-002/source-evidence.md (accepted backend Stop row on build 58a4167fa05832fc22ba30b233afcfcd59151f4c).
