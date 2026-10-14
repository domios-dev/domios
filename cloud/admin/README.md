# Domios admin console

The Next.js app reads through the admin data interface. Only the server adapter uses the Admin SDK or server collections. Browser sign-in uses Auth with in-memory persistence and discards the ID token after exchanging it for an eight-hour session. Every protected page, API read, and server action verifies the session with revocation checking and requires both its boolean admin claim and the user's current admin claim. Missing, expired, revoked, disabled, deleted and non-admin sessions return 403.

Entitlement changes require a reason and an existing user. A transaction replaces entitlements/{uid} and creates a new adminAudit document with actor uid, reason, action, timestamp and complete before/after records. There is no audit update/delete API. Firestore rules deny all browser reads and writes to these collections, including browsers authenticated as admins. Grants have status active, period end 0 (manual indefinite grant) and source admin; revocations preserve the plan and set status revoked. Billing uses the same production collections. The audit history on user detail shows the persisted result after the action redirects.

The users list loads one Firebase Auth page at a time (25, 50 or 100 accounts) and keeps a page-token stack for Next and Previous. Profile checks read only the loaded users' profile documents. Provider, disabled and has-profile filters apply only to that page; the range refers to the unfiltered page. Refresh reloads the current page. Exact email/uid search remains available separately. Detail includes identity, providers, Auth status/timestamps, entitlement provenance and collapsed read-only claims.

Disable/enable and admin role changes use an inline review and confirmation form naming the current email and uid, with a required reason. Server actions verify current admin access and the confirmed target, update Auth, revoke refresh tokens and append an adminAudit entry with before/after snapshots. Claims are merged. Self disable/enable and self admin revocation are refused; removing the last enabled admin is refused by a paginated Auth scan. This simple check has no lock across concurrent administrators. Auth mutations and Firestore audit creation are sequential, not an atomic cross-service transaction; failed requests must be inspected before retrying. Revoked targets need to sign in again to obtain current claims.

## Owner setup and first admin

1. In the account project's Auth console, enable Email/Password and create the owner's account. Record its uid. Enable Firestore and deploy cloud/firestore.rules.
2. In cloud/admin, run npm ci. Provide ADMIN_PROJECT_ID and trusted owner credentials via GOOGLE_APPLICATION_CREDENTIALS (an absolute path to a service-account JSON) or ADMIN_SERVICE_ACCOUNT (the JSON itself).
3. Run npm run set-admin -- <uid> once. This reads the existing claims and merges admin: true, preserving beta and every other claim. Run the bootstrap while no other process is editing that user's claims: the Auth claims API has no conditional update. Subsequent role changes use the user detail confirmation form.
4. Sign in again to obtain a token with the new claim. To remove another admin's access, use the user detail action and provide a reason. Current claims are checked on every protected request.

## Vercel

Create a Next.js Vercel project with root directory cloud/admin, Node 22, install command npm ci and build command npm run build. Copy .env.example values into the project environment for preview and production. ADMIN_SERVICE_ACCOUNT is a server secret; never give it a NEXT_PUBLIC prefix. The three NEXT_PUBLIC_AUTH values are the browser's public Auth configuration. ADMIN_PLANS is the comma-separated allowlist of manual plan ids; match the billing plan configuration.

Set ADMIN_ORIGIN to the exact HTTPS deployment origin for each environment. For a generated preview URL, set it to that URL before testing, or omit it to require the origin of the incoming request. Add the deployment domain to Auth's authorized domains. Leave ADMIN_PROVIDER=firebase; local and fake providers are refused whenever VERCEL is set or NODE_ENV is production. Cookies always use __Host-admin, HttpOnly, Secure, SameSite=Lax, Path=/ and no Domain. Session creation and deletion also require a same-origin Origin header; Next.js server actions apply their origin protection and independently verify the session.

No deployment is performed by npm scripts. The owner must supply the project credentials and Vercel access.

### Deployed project

The Vercel project is `domios-admin` in `gandry90-3616s-projects`, project ID
`prj_4Fezu4C4VYFn6NbqjG8VN06vIKcj`. Its root is `cloud/admin`, framework is
Next.js, Node version is `22.x`, install command is `npm ci`, and build command
is `npm run build`. Deploy from the repository root after linking this project.
The root login page is public; user pages and APIs still require an admin session.

Configured in both Preview and Production: `ADMIN_PROVIDER`, `ADMIN_PROJECT_ID`,
`ADMIN_PLANS`, `NEXT_PUBLIC_AUTH_API_KEY`, `NEXT_PUBLIC_AUTH_DOMAIN`, and
`NEXT_PUBLIC_AUTH_PROJECT_ID`. The Firebase project is `domios-d9357`.
`ADMIN_ORIGIN` is omitted so same-origin checks use the incoming HTTPS origin.
Confirm the manual plan allowlist against the billing configuration before grants.

The server credential is `ADMIN_SERVICE_ACCOUNT`. The owner must create the
shared Bitwarden entry `domios-firebase-admin-sa`; on BLACKSTAR, pipe its value
directly into `vercel env add ADMIN_SERVICE_ACCOUNT` for Preview and Production
using stdin, then redeploy. Never put the credential in shell arguments, source,
deployment logs, or reports. The initial deployment has no service credential.
Blaze/Firestore setup, authorized Auth domains, and first-admin bootstrap must
be completed before the authenticated search and persisted grant/revoke check.

Verified Node 22 preview: `https://domios-admin-53hamj6qt-gandry90-3616s-projects.vercel.app`.
Production alias: `https://domios-admin.vercel.app`.
Production deployment: `https://domios-admin-1nvq1d5sm-gandry90-3616s-projects.vercel.app`.
Vercel project: <https://vercel.com/gandry90-3616s-projects/domios-admin>.
The first deployment was automatically assigned to Production by Vercel;
subsequent previews must explicitly use `--target preview`.

The preview and production login pages load over HTTPS with status 200. A signed-out
`GET /api/users?q=x` returns 403 with `{"error":"Admin access required"}` and
no user data. The deployment build succeeds, 30 focused unit/request/action/local
tests pass, and route generation plus TypeScript checking pass. Authenticated
search and the two persisted `adminAudit` entries from grant/revoke remain
unverified until the owner setup and credential gates above are complete.

## Account-free development

Run npm ci, then ADMIN_PROVIDER=local npm run dev in an authorized dev utility shell. Visit the HTTPS localhost URL printed by Next (trust its development certificate). Secure cookies require HTTPS. Sign in with admin@localhost / local-admin, search user@localhost or local-user, grant a plan with a reason, then revoke it. Refresh the detail page to see both audit entries and the revoked entitlement. Restarting the server preserves the result in .local/admin.json. Override ADMIN_LOCAL_PATH, ADMIN_LOCAL_SECRET and ADMIN_LOCAL_PASSWORD for a private local setup. The local provider supports one development server; it is not a distributed production store. It only accepts session exchange from loopback origins.

The fake provider implements the same interface with injectable identities and in-memory records for unit tests.

## Verification

Canonical QA, from cloud/admin:

- npm ci
- npm test: diagnostics, unit/request/action/local/render tests and Auth/Firestore emulator tests; every selected test must pass with no skipped emulator coverage. Java 21+ is required for the Firestore emulator. On this Mac, prepend /opt/homebrew/opt/openjdk@21/bin to PATH; the system Java launcher does not discover Homebrew automatically. The emulator config lives beside the shared rules in cloud/firebase.admin-test.json because Firebase refuses rules paths outside its project root. The test runner uses ports 29099, 28080, 24400 and 24500, separate from cloud/functions.
- npm run check: Next generates route types, then TypeScript exits 0.
- npm run build: Next compiles all routes and exits 0. Production configuration is lazy, so a build needs no service credentials.

Focused tests: npm run test:unit; npm test -- test/emulator.test.ts. The emulator test proves bounded Auth pages/profile lookups, persisted disable/enable and merged role claims, token revocation calls and append-only account audit, alongside entitlement transaction, bootstrap and browser rules coverage.

After owner setup, deploy a Vercel preview, sign in as the first admin, search a test user by email and uid, inspect metadata and billing link, then grant and revoke with reasons. Pass only when a refreshed user detail shows the final revoked record and both audit entries with the correct actor, reason and before/after. A signed-out or non-admin GET /api/users?q=<uid> and GET /users/<uid> must return 403 without user data; submitting a captured grant action without an admin session must also be denied. This live check is deferred until the owner provides external accounts.
