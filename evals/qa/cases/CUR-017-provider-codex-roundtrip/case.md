# CUR-017 — Codex chat backend provider round-trip

## Purpose

Prove Codex chat_backend accepts one owned prompt and returns one exact answer in its own session.

## Covers

Plan task keys: task_tfrk57 — rebuilt 820 identical-send pair passed for Codex chat_backend 3471eb20 before and after reload; earlier accepted provider-sweep receipt also exists.

Defects: Provider launch appears ready without a reply, reply lands in another session, or source IDs duplicate/collapse.
## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web
- **Platforms:** linux, macos, windows
- **Providers:** codex
- **Grants:** provider-model-budget, owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Launch or select one owned Codex chat_backend tab and record session identity.
2. Send one unique marker and require one user and one assistant row in the same session.
3. For duplicate-send extension, use CUR-001 and require two distinct same-text turns before and after reload.

## Oracle

PASS — The intended Codex session returns one exact assistant answer for one unique user turn.

FAIL — The action is lost, duplicated, misrouted, or reported complete without the required final state.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_tfrk57; target-check/evals/qa/2026-10-18-820f8521/CUR-017/source-evidence.md.
