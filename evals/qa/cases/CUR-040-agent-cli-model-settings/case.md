# CUR-040 — Agent CLI model and speed changes

## Purpose

Exercise agent model, --list-speeds, --speed, switch-provider with model/effort, and busy refusal through documented agent CLI commands.

## Covers

Plan task keys: Owner-mandated post-rebuild cell; no live task key or execution was supplied.

Defects: CLI accepts unknown state, silently drops model/effort, or switches a busy agent despite a required refusal.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** windows
- **Providers:** claude, codex, grok, opencode
- **Grants:** owned-provider-fixtures, model-change-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use owned ready and busy provider fixtures; discover exact syntax from current CLI help and record raw result/operation ID.

## Steps

1. List speeds and record the exact provider choices.
2. Set one model and one speed, switch provider with explicit model/effort, and verify runtime state after each supported action.
3. On an actually busy fixture, attempt the documented switch once and require the contract's explicit refusal with unchanged session/state.

## Oracle

PASS — Each accepted change is reflected in runtime/session state and a busy switch refuses without mutation.

FAIL — Accepted state is not applied or a busy-refused operation mutates provider/session/effort.

NOT RUN — The current DEV identity, command help, provider fixture, or controlled busy state is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Owner-mandated cell remains NOT RUN until executed.
