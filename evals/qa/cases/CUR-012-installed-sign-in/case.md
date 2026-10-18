# CUR-012 — Installed Firebase sign-in lifecycle

## Purpose

Prove an app-generated sign-in challenge reaches the installed desktop, the correct hosted account completes the flow, and sign-out restores a clean account state.

## Covers

Plan task keys: Migrated from sign-in.md; legacy source evidence spans earlier builds and is not current 1.14.9 execution.

Defects: Challenge mismatch, wrong account, secret leakage, false success, stuck pending state, or a credential attempt without a real supported flow.

Migrated source retained in [assets/legacy-recipe.md](assets/legacy-recipe.md); this is archived recipe context, not a current release verdict.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** web, native
- **Platforms:** windows
- **Providers:** none
- **Grants:** explicit-installed-account-grant, secret-store-access, owned-browser
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use the current TESTING/DEV procedures and the grants listed in catalogue.json. Every live action uses an owned DEV target through the approved helper boundary; absent grants leave the row NOT RUN or BLOCKED. Never infer readiness from elapsed time.

## Fixture

Owner authorizes the exact installed account flow and secret-store retrieval. Preserve URL/query values only in process memory and pin the installed build.

## Steps

1. Start a fresh sign-in from the installed app and require exact state, code challenge, allowed callback, and visible ordinary credential form.
2. Enter credentials only through the owner-approved secret store and visible UI; never log or screenshot secrets.
3. Require the same account UID across app state and Account UI, then sign out and require identity=null and pending=false.
4. Run negative redemption only with an explicitly granted no-live-code backend fixture; a hosted protection refusal does not prove successful sign-in.

## Oracle

PASS — The supported hosted sign-in reaches the same account in the installed app and a UI sign-out leaves identity absent and pending false.

FAIL — The app reports success without identity proof, challenge/callback is mismatched, account identity differs, or secret material leaks.

NOT RUN — No explicit account grant, supported credential flow, safe secret-store access, or installed owner restart is available.

## Evidence to record

Record case ID, UTC time, build SHA, boot ID, platform, surface, owned fixture/session IDs, exact action, raw result, far-end state, and opened evidence paths.

## Cleanup

Save evidence first; close only fixtures created by this run. Preserve uncertain operations for classification and never replay them.

## History of notable runs/defects

No completed run is recorded for this case. See matrix/1.14.9.md for the current release row.
