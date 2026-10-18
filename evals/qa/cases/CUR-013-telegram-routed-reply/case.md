# CUR-013 — Telegram inbound and routed reply

## Purpose

Prove an owner-authorized real Telegram inbound message and its routed response arrive once in the intended far-end thread.

## Covers

Plan task keys: Migrated from telegram.md; the original Turing/Keel evidence does not establish a current Telegram acceptance.

Defects: Wrong recipient/thread, duplicate/lost message, false delivery from a bridge ACK, or use of an invented injection endpoint.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

regression

## Surfaces and platforms

- **Surfaces:** cli, team
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** owner-authorized-test-bot, owned-test-chat, far-end-thread-view
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Record the owner-granted bot, poller, test thread, target session, and current DEV build.
2. Have the owner send one unique inbound marker and verify sender/chat/thread provenance in the target own session.
3. Allow one response through the documented route and verify its exact message ID and visible text in the remote Telegram thread.
4. Do not stop the owner poller, borrow a main conversation, or replay uncertain delivery.

## Oracle

PASS — One authorized inbound reaches the intended own session and one response appears in the matching remote thread.

FAIL — The wrong thread receives content, delivery duplicates or disappears, or ACK/log output is used instead of far-end proof.

NOT RUN — The bot/thread/poller grant, consent, or far-end view is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
