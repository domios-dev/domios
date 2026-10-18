# CUR-045 — Teams composer bubble strip

## Purpose

Charter: explore composer bubble strip behavior through interaction sequences on desktop and phone widths.

## Covers

Plan task keys: Owner-mandated Teams Round 3 charter; no task key or execution was supplied.

Defects: Bubbles disappear, duplicate, reorder, cross tabs, or become inaccessible during composition and send.
## Tier

exploratory-charter

## Surfaces and platforms

- **Surfaces:** web, phone, team
- **Platforms:** windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-team-browser
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Mission: exercise bubble add/remove/reorder/overflow and send/cancel flows while tracking source IDs.
2. Scope: disposable Team chat and one owned browser session at desktop and 390x844; no real recipients.
3. Time box: 30 minutes; stop before any unowned delivery.
4. Heuristics: use short and long labels, keyboard and pointer navigation, resize/reload, cancel, and one intentional duplicate text; compare each bubble with the submitted own-session rows.
5. Regression follow-up: a reproducible corrected bubble defect becomes a focused regression case with exact source-ID and DOM oracle.

## Oracle

PASS — Each bubble remains correctly owned, accessible, ordered, and represented once in the submitted conversation.

FAIL — A bubble vanishes, duplicates, crosses tabs, or diverges from the own-session submission.

NOT RUN — No owner-approved disposable Team chat or visible composer fixture is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No run. Round 3 charter is NOT RUN.
