# Domios Account and Analytics

The account and analytics features each own a provider trait; vendors are adapters chosen by config, and each feature ships `local` and `fake` adapters that work with no vendor account.

## Account

`account.provider` selects `local` (without build-time Firebase configuration), `fake` (tests) or `firebase`. Local signs in `local-dev-user` without cloud setup. Provider configuration is read when the account service first starts; restart after changing it. Firebase defaults come from build-time environment variables: DOMIOS_FIREBASE_API_KEY, DOMIOS_FIREBASE_PROJECT_ID, DOMIOS_FIREBASE_APP_ID and DOMIOS_ACCOUNT_REDEEM_URL. Complete values select Firebase by default; missing values select local. The project ID supplies the hosted sign-in URL. Existing account.signin_url, account.redeem_url and account.api_key config values override those defaults; an incomplete Firebase configuration falls back to local. No vendor key is stored in the repository. `account.login_timeout_secs` defaults to 180 (valid range 1–600). Login carries a random state and S256 challenge to the system browser. Both the loopback callback and `domios://auth/callback` require matching state; redemption submits the verifier to the backend. The refresh credential stays in the configured secret store, and the ID token stays in memory. A saved credential is refreshed at startup.

```bash
codeterm account login
codeterm account status
codeterm account logout
```

## Subscription

```bash
codeterm account plan
codeterm account upgrade pro
codeterm account portal
```

The round avatar and chevron immediately left of Settings open the Account menu. Signed-out users can choose Sign in, Create account, Google, Facebook or email. Sign-in and account creation share the system-browser flow; the mode changes its heading. When signed in, its initial opens identity, plan, Manage subscription and Sign out. Both account states offer Community links for Telegram and Discord; the Guide offers the same links, opened in the system browser. Settings > Account shows the same plan, status, period end, cancellation flag and stale marker. Upgrade and Manage subscription open the system browser. The client reads only the signed-in user's server record using the in-memory ID token. Signed-out users get free. A failed fetch retains the last known entitlement marked stale. Sign-in, explicit app focus (at most once per minute), and checkout return refresh the cache; browser return alone never grants access. Returning from checkout to the app bypasses the focus throttle once. Hosted completion may also open `domios://billing/return`.

`billing.provider` defaults to `cloud`; `billing.endpoint` overrides build-time DOMIOS_BILLING_FUNCTIONS_URL and must be an HTTPS base URL ending in `/`. Providers are selected when billing first starts. DEV also supports `local` and `fake`. Local reads `billing.entitlements` on every refresh, so a local config change plus app focus exercises the full plan display without a vendor account. Configure `plan`, `status`, `current_period_end` (Unix seconds), and `cancel_at_period_end`; missing local entitlements return free. Local checkout and portal open the loopback simulator at `billing.origin` (default `http://127.0.0.1:15100/`); run the standalone local accounts backend for its pages. Local config is the entitlement authority for this adapter; simulator payment does not change that config. Release builds refuse explicit dev providers. Without a cloud endpoint, the local fallback returns free and subscription actions report that cloud billing is unavailable.

## Product analytics

Product analytics is off by default; Settings > Privacy or `codeterm analytics status|enable|disable` records opt-in consent in `analytics-settings.json`, and turning it off clears queued events and installation/account identifiers. `DOMIOS_ANALYTICS_MODE` selects `off` (default), `console` (DEV debug logs, no external account) or `amplitude`; `DOMIOS_AMPLITUDE_SERVER_ZONE` is `us` (default) or `eu`; `DOMIOS_ANALYTICS_PROJECT` is `test` (default) or `prod`; set them before starting the daemon. The Amplitude adapter first reads build-time DOMIOS_AMPLITUDE_API_KEY_TEST or DOMIOS_AMPLITUDE_API_KEY_PROD, then reads `domios.amplitude.test.api_key` or `domios.amplitude.prod.api_key` from the secret store and holds the queue while it is missing; analytics status reports vendor_available=false. Every sink shares the consent gate, the closed catalog (feature enums, counts, durations, booleans, numeric versions; never paths, prompts, names or terminal output), the persisted queue and the bounded retry policy.

## Local API

Local callers only.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/account/status` | — | Account identity, effective provider and pending flag |
| POST | `/api/account/login` | — | Browser sign-in; optional provider=google/facebook/email and mode=signin/signup query hints |
| POST | `/api/account/logout` | — | Clear credentials and cached entitlements |
| GET | `/api/account/plan` | — | Cached entitlement and stale marker |
| POST | `/api/account/plan/refresh` | — | Explicit focus refresh, throttled to once per minute |
| POST | `/api/account/upgrade` | `{plan}` | Open checkout in the system browser |
| POST | `/api/account/portal` | — | Open subscription management in the system browser |

<!-- revision: 5 -->
