# Domios admin console

The Next.js app reads through the admin data interface. Only the server adapter uses the Admin SDK or server collections. Browser sign-in uses Auth with in-memory persistence and discards the ID token after exchanging it for an eight-hour session. Every protected page, API read, and server action verifies the session with revocation checking and requires both its boolean admin claim and the user's current admin claim. Missing, expired, revoked, disabled, deleted and non-admin sessions return 403.

Entitlement changes require a reason and an existing user. A transaction replaces entitlements/{uid} and creates a new adminAudit document with actor uid, reason, action, timestamp and complete before/after records. There is no audit update/delete API. Firestore rules deny all browser reads and writes to these collections, including browsers authenticated as admins. Grants have status active, period end 0 (manual indefinite grant) and source admin; revocations preserve the plan and set status revoked. Billing uses the same production collections. The audit history on user detail shows the persisted result after the action redirects.

## Owner setup and first admin

1. In the account project's Auth console, enable Email/Password and create the owner's account. Record its uid. Enable Firestore and deploy cloud/firestore.rules.
2. In cloud/admin, run npm ci. Provide ADMIN_PROJECT_ID and trusted owner credentials via GOOGLE_APPLICATION_CREDENTIALS (an absolute path to a service-account JSON) or ADMIN_SERVICE_ACCOUNT (the JSON itself).
3. Run npm run set-admin -- <uid> once. This reads the existing claims and merges admin: true, preserving beta and every other claim. Run the bootstrap while no other process is editing that user's claims: the Auth claims API has no conditional update. The console provides no privilege-escalation operation.
4. Sign in again to obtain a token with the new claim. To remove admin access, use trusted owner tooling to merge admin: false into the user's existing claims. Current claims are checked on every protected request.

## Vercel

Create a Next.js Vercel project with root directory cloud/admin, Node 22+, install command npm ci and build command npm run build. Copy .env.example values into the project environment for preview and production. ADMIN_SERVICE_ACCOUNT is a server secret; never give it a NEXT_PUBLIC prefix. The three NEXT_PUBLIC_AUTH values are the browser's public Auth configuration. ADMIN_PLANS is the comma-separated allowlist of manual plan ids; match the billing plan configuration.

Set ADMIN_ORIGIN to the exact HTTPS deployment origin for each environment. For a generated preview URL, set it to that URL before testing, or omit it to require the origin of the incoming request. Add the deployment domain to Auth's authorized domains. Leave ADMIN_PROVIDER=firebase; local and fake providers are refused whenever VERCEL is set or NODE_ENV is production. Cookies always use __Host-admin, HttpOnly, Secure, SameSite=Lax, Path=/ and no Domain. Session creation and deletion also require a same-origin Origin header; Next.js server actions apply their origin protection and independently verify the session.

No deployment is performed by npm scripts. The owner must supply the project credentials and Vercel access.

## Account-free development

Run npm ci, then ADMIN_PROVIDER=local npm run dev in an authorized dev utility shell. Visit the HTTPS localhost URL printed by Next (trust its development certificate). Secure cookies require HTTPS. Sign in with admin@localhost / local-admin, search user@localhost or local-user, grant a plan with a reason, then revoke it. Refresh the detail page to see both audit entries and the revoked entitlement. Restarting the server preserves the result in .local/admin.json. Override ADMIN_LOCAL_PATH, ADMIN_LOCAL_SECRET and ADMIN_LOCAL_PASSWORD for a private local setup. The local provider supports one development server; it is not a distributed production store. It only accepts session exchange from loopback origins.

The fake provider implements the same interface with injectable identities and in-memory records for unit tests.

## Verification

Canonical QA, from cloud/admin:

- npm ci
- npm test: unit/request/local tests and six emulator tests; every selected test must pass with no skipped emulator coverage. Java 21+ is required for the Firestore emulator. The test runner uses ports 29099, 28080, 24400 and 24500, separate from cloud/functions.
- npm run check: TypeScript exits 0.
- npm run build: Next compiles all routes and exits 0. Production configuration is lazy, so a build needs no service credentials.

Focused tests: npm run test:unit; npm test -- test/emulator.test.ts. The emulator test proves persisted grant/revoke records, audit collision rollback, concurrent audit continuity, first-admin claim merge and browser rules denial.

After owner setup, deploy a Vercel preview, sign in as the first admin, search a test user by email and uid, inspect metadata and billing link, then grant and revoke with reasons. Pass only when a refreshed user detail shows the final revoked record and both audit entries with the correct actor, reason and before/after. A signed-out or non-admin GET /api/users?q=<uid> and GET /users/<uid> must return 403 without user data; submitting a captured grant action without an admin session must also be denied. This live check is deferred until the owner provides external accounts.
