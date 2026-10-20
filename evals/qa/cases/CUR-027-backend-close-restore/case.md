# CUR-027 — Close and restore the same provider session

## Purpose

Prove a closed tab can be restored to the same provider session and complete a fresh exact turn.

## Covers

Plan task keys: The 820 rebuilt report supersedes the first strict fresh-launch deadline: Codex tab 3471eb20 was absent after close and restored as 522a6051 with the same session, then completed a round-trip after explicit selection.

Defects: A restored sidebar row is treated as a loaded session, session identity changes, or input is replayed during restore.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, cli
- **Platforms:** linux, macos, windows
- **Providers:** codex
- **Grants:** owned-provider-fixture, owned-tab-close
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Record the source tab/session and close only that owned tab once.
2. Require the original tab to be absent, restore it, explicitly select the restored tab, and wait for agent_ready/idle.
3. Send one new marker and prove the same provider session returns one answer; do not replay a prior input.

## Oracle

PASS — The restored tab has the same session and returns one fresh answer after an explicit select.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-820f8521/CUR-027/source-evidence.md (initial observer deadline retained; rebuilt same-session round-trip proved end state).
