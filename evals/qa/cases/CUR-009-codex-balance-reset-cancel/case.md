# CUR-009 — Codex balance and reset cancellation

## Purpose

Distinguish account credits from rate-limit reset credits and prove Cancel never consumes a reset.

## Covers

Plan task keys: Migrated from codex-usage-reset.md; no plan task key was recorded.

Defects: Balance labels conflate separate values, a reset is consumed by Cancel, or account movement is misattributed to the QA run.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** linux, macos, windows
- **Providers:** codex
- **Grants:** dev-browser-auth, owned-codex-runtime, usage-reset-observer
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use an owned Codex runtime, visible Account UI, and scoped request observer on the DEV reset path. Never authorize reset confirmation.

## Steps

1. Read the raw owned-runtime balance and compare it with the Account credits label.
2. Open usage details and read reset count and next expiry separately.
3. Open the reset confirmation, verify its scope and cancel control, then choose Cancel once and require the dialog to close with zero consume requests.
4. Never choose Confirm reset. Record unavailable usage/reset data as NOT RUN.

## Oracle

PASS — Account balance and reset count/expiry are distinct and accurate; Cancel closes confirmation with zero consume requests and no attributable count change.

FAIL — The labels are conflated, Cancel consumes a reset, or a reset/expiry change is attributed without supporting request evidence.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
