# CUR-047 — CLI smoke and monkey sequence

## Purpose

Charter: explore CLI consistency, odd inputs, concurrency, and safe sequential command behavior.

## Covers

Plan task keys: Owner-mandated CLI smoke/monkey charter; task_ty5ff6 is a separate accepted navigation case and does not prove this CLI session.

Defects: Inconsistent help/--json output, unsafe exit codes, lost refusal context, concurrency corruption, or accidental repeated operation.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** cli
- **Platforms:** windows
- **Providers:** none
- **Grants:** owned-dev-caller, disposable-workspace
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: find one reproducible CLI contract or state inconsistency with a minimal safe sequence.
2. Scope: read-only commands and disposable DEV resources; no production, destructive owner data, or blind retry.
3. Time box: 45 minutes; record exact argv, raw exit, build/boot, own target, and before/after state.
4. Heuristics: compare help with --json, odd/missing values, exit codes, refusal messages, bounded concurrent reads, and short monkey sequences that create then explicitly clean owned resources.
5. Regression follow-up: turn a fixed finding into a deterministic regression case and preserve the charter evidence pointer.

## Oracle

PASS — Selected commands match documented output/state and every refusal leaves owned state unchanged.

FAIL — A reproducible contract mismatch, state corruption, or unauthorized mutation occurs.

NOT RUN — No owner-granted QA caller/workspace is available for this charter.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Owner-mandated exploratory charter is NOT RUN.
