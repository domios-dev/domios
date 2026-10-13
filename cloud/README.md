# Domios accounts cloud

Dedicated Firebase project: `domios`. Hosting entry point:
`https://domios-d9357.web.app/signin/?state=...&code_challenge=...&redirect_uri=...`.
Functions v2 run on Node 22 in `us-central1`. This directory is independent of the desktop build.

## Owner setup

1. In the [Firebase console](https://console.firebase.google.com/), create or select project ID `domios`, attach a billing account (Blaze), and create Firestore in production mode in the chosen permanent database location. The project ID is globally unique: if unavailable, choose the dedicated ID and update `.firebaserc`, the web config and URLs below together. Never point this project at an existing application's database.
2. In Project settings → General → Your apps, register a **Web** app named `Domios sign-in`. Export its values as `DOMIOS_FIREBASE_API_KEY`, `DOMIOS_FIREBASE_PROJECT_ID`, and `DOMIOS_FIREBASE_APP_ID`, then run `node cloud/write-signin-config.mjs` from the repository root. The web API key and App Check site key are public configuration; provider secrets and service-account private keys never belong in Hosting.
3. In Authentication → Get started → Settings → Authorized domains, add `domios-d9357.web.app` and confirm `domios-d9357.firebaseapp.com` is present. Add a custom Hosting domain only if actually using it. The desktop's loopback IP and `domios` scheme are **not** Firebase authorized domains or OAuth redirect URIs. The system browser signs into the hosted web app; the daemon receives only its bound desktop code.
4. In Authentication → Sign-in method, enable **Google** and select the support email. In [Google Cloud console](https://console.cloud.google.com/) → Google Auth Platform, configure the app name, support/developer email and audience; add owner/test users while in Testing. Publish the consent screen when ready. Under Clients (or APIs & Services → Credentials), select Firebase's Web OAuth client. Set Authorized JavaScript origins to `https://domios-d9357.web.app` and `https://domios-d9357.firebaseapp.com`; set Authorized redirect URIs to **`https://domios-d9357.firebaseapp.com/__/auth/handler`**. Use the Web client ID/secret in Firebase's Google provider configuration if entering a client manually. No desktop OAuth client or embedded desktop client secret is needed. The web configuration uses `authDomain: "domios-d9357.firebaseapp.com"`.
5. Create a [Meta developer app](https://developers.facebook.com/apps/) with the Facebook Login use case. Under app settings, set App Domains to `domios-d9357.web.app` and `domios-d9357.firebaseapp.com`, Website URL to `https://domios-d9357.web.app/`, contact email, privacy policy URL and data deletion instructions URL. In Facebook Login → Settings, enable Client OAuth Login and Web OAuth Login, require HTTPS and strict redirect matching, and add Valid OAuth Redirect URI **`https://domios-d9357.firebaseapp.com/__/auth/handler`**. Copy the Meta **App ID** and **App Secret** into Firebase Authentication → Sign-in method → Facebook and enable it. Use app administrators/developers/testers in Development mode. Complete Meta's required verification/review and switch to Live before admitting other accounts. Request only the default identity/email scope; email may be absent and is not used to authorize redemption. See [Firebase's Facebook setup](https://firebase.google.com/docs/auth/web/facebook-login).
6. Enable **Email/Password**, then enable **Email link (passwordless sign-in)** in Firebase Authentication → Sign-in method. Configure the sign-in email template sender/display name. The page uses a same-origin `/signin/` continuation URL with the three non-secret desktop challenge fields; it never includes the PKCE verifier, custom token or email in that URL. Confirming a link on another browser requires re-entering the email. Firebase's email action link is handled only by Firebase Auth, never forwarded to the daemon. See [email-link setup](https://firebase.google.com/docs/auth/web/email-link-auth).
7. Apple is wired through `OAuthProvider("apple.com")` but disabled by `appleEnabled: false`. Leave the Firebase provider disabled. To enable later: in Apple Developer, enable Sign in with Apple on a primary App ID, register a Services ID and associate it with that App ID, register `domios-d9357.firebaseapp.com` as the domain and `https://domios-d9357.firebaseapp.com/__/auth/handler` as the return URL, create a Sign in with Apple key, then enter Services ID, Apple Team ID, Key ID and private key in Firebase's Apple provider. Enable the provider and set `appleEnabled: true`. If using Apple's private email relay, register Firebase's sender (`noreply@domios-d9357.firebaseapp.com`, or the customized sender). See [Firebase's Apple setup](https://firebase.google.com/docs/auth/web/apple).
8. In Google Cloud reCAPTCHA Enterprise, enable the API and create a Website score-based key restricted to the two Hosting domains. In Firebase → App Check, register the Web app with **reCAPTCHA Enterprise** and that site key. Put the public site key in `appCheckSiteKey`. The callable has `enforceAppCheck: true` from its first deployment; do not disable it to troubleshoot. See [App Check enforcement](https://firebase.google.com/docs/app-check/cloud-functions).
9. Enable IAM Service Account Credentials API. Grant the Functions v2 runtime service account (default `PROJECT_NUMBER-compute@developer.gserviceaccount.com`, unless explicitly overridden) `roles/iam.serviceAccountTokenCreator` **on itself**, so the Admin SDK can sign custom tokens without a downloaded key. Ensure its Firestore access (`roles/datastore.user`) is present. Keep deployment access with the owner. Neither function logs request bodies, codes, verifiers or tokens.
10. In Firestore → Time-to-live, enable TTL on `expires_at` for collection groups `desktopLoginCodes` and `desktopLoginRateLimits`. TTL is cleanup only; server transactions enforce expiration even before deletion.

## Build, verify and deploy

Prerequisites: Node **22**, npm, Java **21 or later** for the Firestore emulator,
and the Firebase owner login. Tests use `demo-domios` and local emulators only;
they do not need owner credentials or access the production project.

```sh
npm --prefix cloud/functions ci
npm --prefix cloud/functions test
node cloud/write-signin-config.mjs
# Fill in the public web configuration and App Check site key as described above.
npm --prefix cloud/functions run build
cd cloud
npx --prefix functions firebase login
npx --prefix functions firebase deploy --project domios-d9357 --only firestore:rules,functions,hosting
```

`npm test` compiles Functions and bundles the page, then executes
`firebase emulators:exec --config ../firebase.json --project demo-domios --only auth,firestore,functions "vitest run"`.
The happy-path oracle exchanges the returned custom token through the Auth
emulator and checks the authenticated UID; a 200 alone is not its pass condition.
Tests prove concurrent single use, TTL, all challenge mismatches, creation
redirect/auth/App Check refusals, generic redemption failures, IP throttling,
and rules denying reads/writes to server-only and unknown collections.
The emulator uses unsigned test App Check tokens; production signature validation
is performed by Firebase's enforced callable middleware, not simulated by that test.
Production OAuth, email delivery and reCAPTCHA require the owner configuration
and a system-browser check after deployment. No CI job is added without evidence
that a cold emulator run fits the two-minute `light` runner limit.

## Desktop protocol

The daemon retains a cryptographically random PKCE verifier (43–128 RFC 7636
characters) and a random base64url state (16–256 characters). It sends
`code_challenge = base64url(SHA256(verifier))`, the state and an exact redirect
URI to `/signin/`. S256 is the only supported challenge transform.
Redirects are exactly `http://127.0.0.1:<1–65535>/callback` (no leading zeroes,
credentials, query, fragment or additional path) or `domios://auth/callback`.
IPv6, `localhost`, HTTPS and other custom schemes are refused.

After Firebase sign-in, the page calls `createDesktopLoginCode` and navigates to
`redirect_uri?code=...&state=...`. The 256-bit code is stored only under its SHA256
hash, expires after 120 seconds, and requires the daemon-held verifier to redeem.
The browser uses in-memory Firebase Auth persistence and signs out before the
callback. No Firebase custom token or provider access token reaches the callback URL.

The daemon POSTs JSON to the **direct** endpoint
`https://us-central1-domios.cloudfunctions.net/redeemDesktopLoginCode`:

```json
{"code":"...","code_verifier":"...","state":"...","redirect_uri":"http://127.0.0.1:45123/callback"}
```

Success: `200 {"custom_token":"..."}` with `Cache-Control: no-store`.
Every application redemption failure: `400 {"error":"Unable to complete desktop sign-in"}`.
Use `custom_token` to sign into Firebase Auth; do not log it or put it in a URL.
The code is consumed in one Firestore transaction before signing the custom token.
If token signing or response delivery fails, start a new desktop sign-in; never
replay a consumed code. Firebase's callable middleware uses its own protocol
errors for missing/invalid Firebase Auth or App Check credentials.

Rate limits are transactional, shared across instances, and allow 30 redemption
attempts per IP per 60-second window, including malformed and failed attempts.
Only the SHA256 of the IP is persisted. Direct Google ingress appends the client
and load-balancer addresses to X-Forwarded-For; the handler ignores caller-supplied
prefixes and fails closed without that trusted suffix. Do not add a Hosting
rewrite, CDN or proxy to this endpoint without adapting and verifying that ingress
contract. [Google documents the appended header format](https://cloud.google.com/load-balancing/docs/https#x-forwarded-for_header).

Firestore profile documents support only `displayName` (string, at most 100
characters) and `photoURL` (string, at most 2048 characters). Render them as
untrusted content. Clients can access only their own profile; nested user docs,
code/rate records, entitlements, all billing/admin collections and all unmatched
paths are denied. Server code uses IAM and the Admin SDK, which bypass these rules.

## Provider boundary and local DEV

The pure `CodeExchange` model owns PKCE, hashing, the 120-second deadline and
single-use validation. Its `CodeStore` contract requires atomic consume and
rate counting; `TokenMinter` supplies the account token. Production wiring lives
only in `src/adapters/firebase/`. The adapter factory selects stores and minters;
`package.json` selects the production entry point (`main`) or local entry point
(`scripts.dev`), so deploying never implicitly switches to a test or local store. The `fake` adapter supplies inspectable records
and tokens for deterministic tests. The `local` adapter uses a synchronous
in-process store and signed local account tokens; restarts intentionally invalidate
all local codes, sessions and tokens. It is for single-process DEV, never deployment.

The page talks only to `AuthClient`; its config selects the production or local
adapter. The fake browser adapter implements that same interface for tests and is
not a production configuration option.

```sh
npm --prefix cloud/functions ci
npm --prefix cloud/functions run dev
```

This starts a loopback-only local account service at `http://127.0.0.1:15100`,
serves the bundled sign-in page, and supplies `{ "provider": "local" }` configuration.
Set `DOMIOS_ACCOUNTS_PORT` to choose another free port. Open `/signin/` with the
same desktop challenge query and select **Continue with local account**.
The daemon uses `/api/redeemDesktopLoginCode` on this service and exchanges the
returned token at `/api/account` to observe `uid: "local-user"`. The local adapter
requires no Firebase project, OAuth client, email service, reCAPTCHA or emulator.
It rejects foreign Origin/Host headers, and uses the same code-exchange model.
The focused local integration test proves sign-in → code → redeem → authenticated
account and rejects replay and token tampering.

Transport-level failures rejected by Google/Firebase's request parser before a
function is invoked (invalid JSON syntax or platform body-size limits) use the
platform error response; the generic JSON error applies to requests reaching the
redemption handler. Callers must send a JSON object as specified above.

## Paddle billing setup and verification

Billing is selected with `BILLING_PROVIDER=paddle` and `BILLING_MODE=test` (Paddle
sandbox) or `live`. `createCheckoutSession` accepts only `{plan: "pro"}` or
another configured plan key. It verifies the Firebase ID token, derives the uid,
creates a Paddle transaction for one allow-listed price, and returns `{url}` on
the configured default payment link. `createPortalSession` verifies the same
identity, loads its stored Paddle customer, and creates a fresh server-side
`POST /customers/{customer_id}/portal-sessions` session. Desktop Upgrade and
Manage already consume these callable URLs and open the system browser; the
`codeterm-billing` crate remains vendor-free.

Store this server configuration in `test_billing/config` for sandbox or
`billing/config` for live. Values shown are placeholders, not credentials:

```json
{
  "plans": {"pro": "pri_REPLACE_WITH_26_CHARACTER_ID"},
  "hostedOrigin": "https://domios.dev",
  "checkoutUrl": "https://domios.dev/checkout",
  "paddleClientToken": "live_REPLACE_WITH_PUBLIC_CLIENT_TOKEN"
}
```

`checkoutUrl` must be an HTTPS URL without query or fragment, on the approved
website/default payment link. Sandbox uses its own price catalog and a `test_`
client token. The website and Paddle Website approval must exist before even a
sandbox transaction journey can be proven for that domain. The website team owns
`web/` and `/checkout`, pricing, terms, privacy, and refund pages; this task ships
only the framework-free checkout contract.

Three credential roles must remain distinct:

- `PADDLE_SANDBOX_API_KEY` / `PADDLE_LIVE_API_KEY`: Secret Manager, server-only.
  The owner's live vault reference is `domios-paddle-live-api-key`.
- `paddleClientToken`: public checkout configuration, never an API key. The
  owner's live vault reference is `domios-paddle-live-client-token`.
- `PADDLE_SANDBOX_WEBHOOK_SECRET` / `PADDLE_LIVE_WEBHOOK_SECRET`: Secret Manager,
  separate endpoint secrets. Provision using `firebase functions:secrets:set NAME`.

Sandbox credentials and both webhook endpoint secrets are owner prerequisites.
Firebase Blaze is required for deployment. Do not deploy, mutate the Paddle
dashboard, or perform a live charge/subscription as part of local verification.

### Website checkout contract

The importable module is `cloud/checkout/paddle.ts`; copy/import it into the
website build without React dependencies. It loads the official
`https://cdn.paddle.com/paddle/v2/paddle.js` script and waits for its load event.
Exact public config names and values are:

```ts
const config = {
  paddleEnvironment: "production", // "sandbox" for test mode
  paddleClientToken: "live_REPLACE_WITH_PUBLIC_CLIENT_TOKEN",
};
await openPaddleCheckout(config, undefined, {
  onCompleted(transactionId) { showSuccessAndOpenDomiosButton(transactionId); },
  onClosed() { showCanceledCheckout(); },
  onError(error) { showCheckoutError(error.message); },
});
```

`openPaddleCheckout` calls `Paddle.Environment.set(config.paddleEnvironment)`,
then `Paddle.Initialize({token: config.paddleClientToken, eventCallback})`, then
`Paddle.Checkout.open({transactionId})`. Its default transaction id comes from
`_ptxn` in the current URL, so both Paddle's default-payment-link flow and the
server-returned `https://domios.dev/checkout?_ptxn=txn_...` work. A page that
already has a server-created id may call `openPaddleCheckout(config, transactionId)`.
Missing/invalid ids and mismatched token environments fail before loading Paddle.
The page should show a retryable error for a rejected promise and offer a return
to the app. The returned promise signals only that checkout was opened. `onCompleted(id)`
receives a matching `checkout.completed` event once; only then show success and
the Open Domios button. `onClosed()` receives `checkout.closed` only if checkout
has not completed, once. `onError(error: Error)` surfaces checkout and payment
errors. Pass these typed callbacks as the third argument. Completion is a UI
signal only and is never an entitlement grant; refresh
`getEntitlements` and wait for the persisted verified webhook result.

`billingCheckoutConfig` is an unauthenticated GET endpoint returning exactly
`{paddleEnvironment, paddleClientToken}` for the deployment's mode, with
`Cache-Control: no-store`. It has no mounted secrets and never returns an API key
or webhook secret. The Vercel website may expose these two public fields as build
configuration or proxy this endpoint on its own origin. It must select the same
environment as the function deployment. No cross-origin browser fetch is required.

Merge these additions into the website's CSP (keep its existing sources):

```text
script-src: https://cdn.paddle.com
frame-src: https://buy.paddle.com https://sandbox-buy.paddle.com
connect-src: https://*.paddle.com
img-src: https://*.paddle.com data:
```

These cover the SDK, live/sandbox checkout frames and Paddle services. Payment
provider resources inside Paddle's cross-origin frames use the frame's own CSP.
No `unsafe-eval` or inline script exception is needed by the module. Serve the
module from the website's own bundle. Use a no-referrer policy on `/checkout`.

The website's return link is `domios://billing/return`, optionally
`domios://billing/return?status=completed` or `?status=canceled`. The sole allowed
parameter is `status`, an opaque 1–64-character ASCII alphanumeric/underscore/hyphen
flag for UI copy. Duplicate status, other parameters, fragments, credentials and
ports are refused. It carries no uid, plan, entitlement or redirect target. The
app dispatches one `CheckoutReturned` entitlement refresh, then shows, unminimizes
and focuses its main window; the normal focus refresh is throttled by that read.
The status never grants access. A webhook may arrive after the browser return;
until a subsequent normal refresh sees it, the app retains its last verified
entitlements. No retry timer is installed. Offer this as a user-activated return
link after completion/cancel, rather than assuming a browser may open a custom
scheme without a gesture.

The server fixes `items: [{price_id, quantity: 1}]`, `collection_mode: automatic`,
`checkout.url`, and `custom_data.uid` from the verified ID token. It adds
`custom_data.billing_binding`, an HMAC bound to uid and mode, to prevent a
transaction created with the public client token from forging an account grant.
Do not modify the transaction's custom data in Paddle.js. Paddle copies transaction
custom data to the new subscription. Only verified webhook custom data can bind
a previously unknown customer; conflicting account/customer bindings are refused.

### Webhooks and proof

Register `/paddleWebhook` for subscription.created, activated, updated, canceled,
past_due; transaction.completed, payment_failed; adjustment.created and updated.
The handler verifies `Paddle-Signature` over the exact raw bytes (`ts:body`,
HMAC-SHA256, constant-time comparison, five-second timestamp tolerance), supports
multiple `h1` values, and uses the selected mode's endpoint secret. Paddle event
payloads have no live-mode field: separation is enforced by distinct secrets,
mode-specific API origins, and a store refusing events of another mode.

Event ids and entitlement updates share one Firestore transaction using
`test_billingEvents`/`billingEvents`. Sandbox also uses `test_billingCustomers`
and `test_entitlements`; it never writes live collections. Older events cannot
replace newer state. Approved partial/full refunds and chargebacks revoke access;
pending/rejected refunds do not. Revocation remains sticky until audited admin
repair. Unknown customers return an error without recording a grant event,
allowing retry. Transaction events retrieve their subscription from the selected
server API to validate the allow-listed price and billing period.

Run `npm --prefix cloud/functions test -- billing paddle` for focused emulator
proof, and `npm --prefix cloud/functions test` for canonical cloud QA. Java 21+
is required. The runner builds/typechecks once and creates disposable fixture
secrets only when `.secret.local` is absent, then removes them. Fixtures under
`test/fixtures/paddle/` contain no customer data and are signed with a test secret.
Provider API calls are faked. Tests prove persisted lifecycle state, duplicates,
mode isolation, bad signatures, authentication, price allow-list and ownership
refusals, public-token checkout configuration and the website module call contract.
A real sandbox checkout and persisted active `test_entitlements/{uid}` are still
required once the owner supplies credentials and the website is approved.

Local/fake adapters remain test-only and require the emulator outside the local
accounts server. The local `/billing` journey still applies signed local events
and persists in-memory entitlements; restarting local development clears them.

## Deploy-time sign-in configuration

Export DOMIOS_FIREBASE_API_KEY, DOMIOS_FIREBASE_PROJECT_ID and
DOMIOS_FIREBASE_APP_ID from the same named secrets used by app builds, then run
node cloud/write-signin-config.mjs before deploying Hosting. It writes the ignored
cloud/hosting/signin/config.json and exits with a missing-variable message if
any required value is empty. Export the App Check site key before running the writer. Never commit the generated file.
DOMIOS_FIREBASE_APP_CHECK_SITE_KEY is also required: the hosted adapter uses App Check;
DOMIOS_APPLE_ENABLED=true enables Apple sign-in after its provider is configured.
DOMIOS_ACCOUNT_REDEEM_URL and DOMIOS_BILLING_FUNCTIONS_URL configure the desktop
adapters with the deployed HTTPS function URL and HTTPS base URL ending in /.

Hosted sign-in accepts optional provider=google|facebook|email and
mode=signin|signup query hints. A supported social provider starts immediately;
email focuses its link form. Unknown/disabled hints fall back to all choices.
Mode changes only the heading, and email continuation preserves it. Firebase
creates accounts on first sign-in; there is no separate sign-up credential flow.
