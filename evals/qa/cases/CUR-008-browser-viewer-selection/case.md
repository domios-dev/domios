# CUR-008 — Browser text selection through the viewer

## Purpose

Prove pointer selection travels through the displayed viewer to the owned remote page.

## Covers

Plan task keys: Migrated text-selection journey from browser.md; the historical Joy attempt selected the wrong content and remains unverified.

Defects: Direct DOM/CDP selection is mistaken for viewer input, wrong text is selected, or evidence comes from an unowned target.
## Tier

regression

## Surfaces and platforms

- **Surfaces:** web, native
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** dev-browser-auth, desktop-viewer, owned-browser
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Prepare one uniquely named, disposable DEV fixture. Record the returned IDs and pin the build and boot before action.

## Steps

1. Open the owned fixture in the visible DEV viewer and record target, viewport, and frame metadata.
2. Drag the intended paragraph through the viewer and wait for the remote selectionchange event on the same owned connection.
3. Compare exact selected text and opened screenshot; do not substitute direct DOM Range or remote CDP input.

## Oracle

PASS — The viewer gesture selects exactly the intended substring in the owned page and the visual selection matches.

FAIL — Selection is empty or differs, or direct page automation is used as viewer evidence.

NOT RUN — A controlled visible viewer, owned Browser target, or exact selection event cannot be observed.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

- Historical result — NOT RUN — `evals/qa/tools/history.md` records the wrong-field selection probe; it is not a dated release verdict.
