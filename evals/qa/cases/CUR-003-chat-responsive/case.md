# CUR-003 — Responsive chat affordances

## Purpose

Prove chat timestamps and compact tool-card presentation remain usable at desktop and narrow touch-sized widths without duplicate controls.

## Covers

Plan task keys: no plan task key was captured for the legacy layout subcase.

Defects: timestamps visible at the wrong widths, expanded tool-card groups, and a duplicate strip of controls. Migrated from chat-send.md step 6.

## Tier

regression

## Surfaces and platforms

- **Surfaces:** phone, web
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** dev-browser-auth, desktop-viewer, dev-fixture-spawn
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the owned DEV Browser and a disposable chat with at least one completed turn and one compact tool-card group. The phone surface here means responsive browser layout at the specified narrow widths; it does not claim physical iOS or Android coverage.

## Fixture

Use a harmless provider-independent fixture conversation with a timestamped message and a compact tool-card group. Record the owned page and the measured viewport rather than assuming a requested width was applied.

## Steps

1. Open the owned DEV chat at a desktop viewport of 1440 CSS pixels and observe the message timestamp on hover.
2. Observe the same chat at touch-sized 768 and 390 CSS pixel widths. Use accessible controls and real pointer/touch behavior; do not infer content from a screenshot alone.
3. Inspect the compact tool-card group and control strip at each viewport after any active animation has completed.

## Oracle

PASS — the timestamp appears on desktop hover and stays hidden at narrow widths; tool cards remain compact and no duplicate control strip appears at all required viewports.

FAIL — a timestamp is persistently visible at narrow widths, a control is missing or duplicated, or tool-card grouping expands contrary to the case.

NOT RUN — no owned Browser, authenticated DEV page, matching fixture, or applicable desktop platform is available. Physical phone coverage is NOT RUN unless a device-specific case and grant are supplied.

## Evidence to record

Record the case ID, platform, UTC time, build SHA, boot ID, actual viewport dimensions, accessible control names and counts, relevant DOM facts, and screenshot paths actually opened. Mark browser emulation separately from physical-device evidence.

## Cleanup

Close only the owned Browser page and fixture tab. Preserve the fixture conversation until evidence is saved, then remove only the disposable workspace under its owner grant.

## History of notable runs/defects

Turing's historical report includes timestamp-width and compact tool-card observations; it is a seed only. The comprehensive wave did not execute this layout check. No live run was made for this catalogue change.
