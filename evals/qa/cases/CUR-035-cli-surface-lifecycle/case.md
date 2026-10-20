# CUR-035 — CLI surface lifecycle and persisted results

## Purpose

Check the supported CLI tab lifecycle, output retrieval, delayed delivery, one-shot schedule cleanup, and exact persisted results.

## Covers

Plan task keys: The accepted comprehensive-ready-lifecycle ledger covers tab new/list/inspect/exec/get-text/stop/restart/close, CLI send and delayed send, memory, cron, Blueprint dry-run, and notification list-only cells; source run was frozen58a and not rerun on 820.

Defects: CLI success without persisted state, wrong tab/session output, delayed message recurrence, or scope-changing commands hidden in a smoke.
## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** windows
- **Providers:** none
- **Grants:** owned-dev-caller, disposable-workspace, scheduled-job-grant
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Run each CLI cell from the owner-authorized QA pad using the published help and exact own caller.
2. Require raw exits and far-end tab/session/job/result state for each operation; keep notification checks list-only.
3. Delete a future one-shot job before it fires; do not trigger unowned work or report proof from help text alone.

## Oracle

PASS — Each selected CLI cell has an exact raw exit and persisted far-end state, with no delayed recurrence or scope leak.

FAIL — A command returns success without the expected state, repeats work, or changes an unowned resource.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — target-check/evals/qa/2026-10-18-58a4167f/CUR-035/source-evidence.md (accepted CLI cells on build 58a4167fa05832fc22ba30b233afcfcd59151f4c; the 820 wave did not rerun them).
