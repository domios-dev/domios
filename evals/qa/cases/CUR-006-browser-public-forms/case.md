# CUR-006 — Built-in Browser forms and clean profiles

## Purpose

Verify real public form navigation, native form states, explicit completion waits, and clean-profile restrictions through the built-in Browser.

## Covers

Plan task keys: Migrated from browser.md; no plan task key was recorded.

Defects: False form completion, missed disabled/readonly/required state, or unauthorized persistent profile changes.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

regression

## Surfaces and platforms

- **Surfaces:** web
- **Platforms:** linux, macos, windows
- **Providers:** none
- **Grants:** dev-browser-auth, owned-browser
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Use an owned Browser session with a clean temporary profile. Use synthetic form values only.

## Steps

Use the [controlled-form segment](../../REPEATABLE-QA.md#controlled-form-subset) for reusable mechanics and retained prototype evidence. It covers only the stated submission subset; public-site navigation, all form flags and nonexistent-profile refusal still need their own observations. Keep automation manual and do not report this segment as a full-case PASS.

1. Submit the public test form with a synthetic value and verify the returned value.
2. Inspect disabled, readonly, selected, and required fields; submit the controlled web form and wait for its explicit completion text.
3. Attempt to open a nonexistent persistent profile and require a refusal with the profile list unchanged.

## Oracle

PASS — Submitted values and form-state flags match the page; completion uses an explicit DOM wait; nonexistent profile selection refuses without changing profiles.

FAIL — The Browser reports an unobserved submission, state flags are wrong, or a refusal changes a persistent profile.

NOT RUN — The required provider, grant, fixture, target, device, or capability is unavailable.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
