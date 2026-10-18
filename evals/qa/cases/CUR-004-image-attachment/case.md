# CUR-004 — Image attachment reaches the intended model

## Purpose

Prove the selected PNG or JPEG bytes reach the intended model once while preserving the prompt and expected opaque image fact.

## Covers

Plan task keys: Migrated from images.md; no task key was recorded for the source journey.

Defects: Text-only fallback presented as image success, duplicate attachment delivery, wrong recipient, or a guessed image description.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** web, phone
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-browser-auth, image-fixture, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use a small synthetic PNG/JPEG with a known non-sensitive distinguishing feature and an owned provider session.

## Steps

1. Attach one fixture image and submit one unique prompt.
2. Verify the own-session request contains the attachment and expected text exactly once.
3. Require an answer that identifies only the expected opaque feature; preserve raw upload and rendered evidence.

## Oracle

PASS — The intended session receives one image plus the prompt and returns the fixture's expected fact.

FAIL — The model receives no image, the image or text duplicates, the wrong session receives it, or the answer contradicts the known fixture.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
