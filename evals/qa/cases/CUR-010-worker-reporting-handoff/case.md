# CUR-010 — Worker reports and independent review handoff

## Purpose

Prove an assigned Worker acknowledges and reports through its own attempt, and a task-less tester reports and reviews only after explicit delegation.

## Covers

Plan task keys: Migrated from worker-kinds.md. task_fn0aoi covers canonical QA/release-manager placement requirements; Round 3 charter cells remain unrun.

Defects: Wrong attempt/session report, false task-less binding, reviewer authority without handoff, or a stale provider session after switch.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** disposable-team-plan, owner-task-delegation, owned-worker-fixtures
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

The owner creates a disposable Team and Plan with one tiny task, Orchestrator, assigned Worker, task-less tester, and explicit review delegation.

## Steps

1. Dispatch once, ACK the current attempt, and report through the Worker's own pad; verify durable parent receipt and task state.
2. Spawn a task-less tester only with owner grant, report without task/status flags, and verify far-end delivery.
3. Require an unrelated review attempt to refuse without state change, then explicitly delegate and verify independent review evidence.
4. If switching providers is included, check tab, task, attempt, and session binding before accepting an ACK.

## Oracle

PASS — The assigned attempt and task-less report reach the parent with exact sender/session identity; unrelated review refuses and handed review persists.

FAIL — A report is lost or misbound, reviewer authority escapes the explicit handoff, or duplicate respawn/provider switch corrupts the attempt binding.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
