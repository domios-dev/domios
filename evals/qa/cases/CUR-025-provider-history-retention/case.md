# CUR-025 — Provider switch preserves readable conversation history

## Purpose

Prove previous conversation items remain distinctly readable after a provider handoff.

## Covers

Plan task keys: task_tfrk57/task_lzwfe9 — the 820 report found the original Claude identical-send pair no longer represented as separate visible/source-ID rows in the current Codex chat; no storage-loss claim is made.

Defects: Embedded transcript text is mistaken for independently retained source rows or message storage loss is inferred without storage evidence.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** owned-provider-fixture
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Send two identical markers to the owned source provider and record their distinct source IDs.
2. Switch provider using CUR-024, then inspect the visible conversation and own-session source IDs.
3. Require both prior messages to remain independently represented; do not infer storage loss from transcript presentation alone.

## Oracle

PASS — Both prior source IDs remain separately visible and correctly associated after handoff.

FAIL — The source IDs are no longer separately represented in the visible/current conversation after handoff.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — FAIL — task_tfrk57; target-check/evals/qa/2026-10-18-820f8521/CUR-025/source-evidence.md (handoff transcript retained text, but original Claude pair did not remain separate rows in current Codex chat).
