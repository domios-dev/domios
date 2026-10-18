# CUR-024 — Provider switch handoff and completion

## Purpose

Prove an in-place UI provider switch hands off the current task and reaches an exact assistant completion in the same tab.

## Covers

Plan task keys: task_lzwfe9 — rebuilt 820 tab-menu Claude-to-Codex handoff has an exact handoff record and assistant receipt.

Defects: Handoff acknowledgement without completion, changed tab/task binding, or provider switch that silently drops the current turn.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, team
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** owned-provider-fixture, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Start one owned turn and switch provider once through the visible tab menu.
2. Require exact handoff receipt, same intended tab/task binding, and the expected assistant final state.
3. Retain early observer errors as evidence; use only a separate end-state receipt to establish completion.

## Oracle

PASS — The declared handoff and its assistant completion are both present with the intended tab/task identity.

FAIL — The switch loses the task, changes the binding unexpectedly, or stops at an ACK without final completion.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_lzwfe9; target-check/evals/qa/2026-10-18-820f8521/CUR-024/source-evidence.md.
