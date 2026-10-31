# CUR-001 — Chat send once and recipient isolation

## Purpose

Prove each authored input reaches the intended provider session once, yields the expected final answer, and appears once. The two-recipient extension checks for cross-talk.

## Covers

Plan task keys: task_tfrk57 — identical-send pairs on Claude PTY 7a41398a and Codex chat_backend 3471eb20, each with two distinct same-text user turns and two answers before and after reload.

Defects: Wrong-session delivery, duplicate or missing turns, false delivery state, collapsed identical messages, or recipient cross-talk.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-browser-auth, dev-fixture-spawn, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Create an owned workspace, DEV Browser, Codex chat_backend, and Claude PTY. Record both tabs, provider sessions, and binding IDs before sending.

## Steps

Follow the [own-session composer segment](../../REPEATABLE-QA.md#own-session-composer-and-linked-reply-segment) for once-only submission and structural completion evidence. An expected final answer means a completed own-session turn plus any required action's far-end evidence, without matching model prose. Intentional identical sends are two authored operations, never retries of uncertainty. The helper alone does not cover rendering, persistence or recipient isolation; automation stays manual.

1. For the CLI surface, send one pad-authored marker with dc --as-tab OWNER send --tab TAB --from-pad marker.md --json. For web, type and submit once in the owned composer.
2. For identical-send coverage, send the same marker twice intentionally. Require two different user IDs, one answer per user ID, and one DOM row for each ID before and after reload.
3. As an extension, send different markers to two owned recipients and prove each own session receives only its intended marker.
4. Classify held or written uncertainty from exact session evidence; never resend.

## Oracle

PASS — Each selected send has one own-session user row and one expected final answer; intentional identical sends remain two distinct rows through reload; recipients do not cross-talk.

FAIL — A send is duplicated, collapsed, lost, misrouted, replayed after reload, or disagrees between source IDs and rendered rows.

NOT RUN — The provider, owned Browser, DEV authentication, fixture grant, or model budget is unavailable; or written uncertainty cannot be classified without replay.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- 2026-10-18 — PASS — task_tfrk57; target-check/evals/qa/2026-10-18-820f8521/CUR-001/source-evidence.md (Claude PTY and Codex chat_backend: two identical sends and two visible turns before and after reload).
