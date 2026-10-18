# CUR-026 — Reconnect without replay

## Purpose

Prove a supported Browser reconnect restores the same session without submitting another user turn.

## Covers

Plan task keys: The 820 lifecycle report records one supported reconnect, one exact chat-reattached event, and unchanged own source IDs on restored tab 522a6051.

Defects: Reconnect creates a new send, replays the previous input, changes session ownership, or produces multiple reattach events.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** linux, macos, windows
- **Providers:** codex
- **Grants:** owned-provider-fixture, dev-browser-auth
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Record same-session user/assistant IDs before one supported reconnect.
2. Wait for one chat-reattached event and inspect the same tab/session after reconnect.
3. Require unchanged source IDs and no additional user turn; do not click Send during reconnection.

## Oracle

PASS — One reattach event restores the same session and each existing source ID remains present once with no new send.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-820f8521/CUR-026/source-evidence.md.
