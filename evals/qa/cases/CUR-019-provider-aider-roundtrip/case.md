# CUR-019 — Aider routed provider round-trip

## Purpose

Prove an authorized routed message reaches Aider and one exact response returns in its own provider session.

## Covers

Plan task keys: task_cjymmf — rebuilt 820 Aider routed round-trip passed; the later unsolicited acknowledgement is a separate incident in CUR-020.

Defects: Routed input is lost, repeated, or the intended Aider answer is not visible in its own session.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** linux, macos, windows
- **Providers:** aider
- **Grants:** provider-model-budget, owned-taskless-caller
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Use a one-time pad-backed send from the owned caller to the owned Aider tab.
2. Require one exact Aider assistant answer and matching own-session receipt.
3. Keep unsolicited later turns separate from the one-turn delivery oracle and classify them with CUR-020.

## Oracle

PASS — One routed marker yields one exact Aider answer in the same session.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_cjymmf; target-check/evals/qa/2026-10-18-820f8521/CUR-019/source-evidence.md.
