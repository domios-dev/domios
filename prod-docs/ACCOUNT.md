# Domios Account and Analytics

The account and analytics features each own a provider trait; vendors are adapters chosen by config, and each feature ships `local` and `fake` adapters that work with no vendor account.

## Account

`account.provider` selects `local` (default), `fake` (tests) or `firebase`. Local signs in `local-dev-user` without cloud setup. Provider configuration is read when the account service first starts; restart after changing it. With `firebase`, `account.signin_url` defaults to `https://domios-d9357.web.app/signin/`. `account.redeem_url` defaults to the Functions v2 HTTPS function URL pattern `https://us-central1-domios-d9357.cloudfunctions.net/redeemDesktopLoginCode`; set it to the URL reported by deployment. `account.api_key` defaults to the public web API key of project `domios-d9357`. `account.login_timeout_secs` defaults to 180 (valid range 1–600). Login carries a random state and S256 challenge to the system browser. Both the loopback callback and `domios://auth/callback` require matching state; redemption submits the verifier to the backend. The refresh credential stays in the configured secret store, and the ID token stays in memory. A saved credential is refreshed at startup.

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

Settings > Account shows the same plan, status, period end, cancellation flag and stale marker. Upgrade and Manage subscription open the system browser. The client reads only the signed-in user's server record using the in-memory ID token. Signed-out users get free. A failed fetch retains the last known entitlement marked stale. Sign-in, explicit app focus (at most once per minute), and checkout return refresh the cache; browser return alone never grants access. Returning from checkout to the app bypasses the focus throttle once. Hosted completion may also open `domios://billing/return`.

`billing.provider` defaults to `cloud`; `billing.endpoint` defaults to `https://us-central1-domios-d9357.cloudfunctions.net/` and must be an HTTPS base URL ending in `/`. Providers are selected when billing first starts. DEV also supports `local` and `fake`. Local reads `billing.entitlements` on every refresh, so a local config change plus app focus exercises the full plan display without a vendor account. Configure `plan`, `status`, `current_period_end` (Unix seconds), and `cancel_at_period_end`; missing local entitlements return free. Local checkout and portal open the loopback simulator at `billing.origin` (default `http://127.0.0.1:15100/`); run the standalone local accounts backend for its pages. Local config is the entitlement authority for this adapter; simulator payment does not change that config. Release builds refuse dev providers.

## Product analytics

Product analytics is off by default; Settings > Privacy or `codeterm analytics status|enable|disable` records opt-in consent in `analytics-settings.json`, and turning it off clears queued events and installation/account identifiers. `DOMIOS_ANALYTICS_MODE` selects `off` (default), `console` (DEV debug logs, no external account) or `amplitude`; `DOMIOS_AMPLITUDE_SERVER_ZONE` is `us` (default) or `eu`; `DOMIOS_ANALYTICS_PROJECT` is `test` (default) or `prod`; set them before starting the daemon. The Amplitude adapter reads `domios.amplitude.test.api_key` or `domios.amplitude.prod.api_key` from the secret store and holds the queue while it is missing. Every sink shares the consent gate, the closed catalog (feature enums, counts, durations, booleans, numeric versions; never paths, prompts, names or terminal output), the persisted queue and the bounded retry policy.

## Local API

Local callers only.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/account/status` | — | Account identity and pending flag |
| POST | `/api/account/login` | — | Browser sign-in |
| POST | `/api/account/logout` | — | Clear credentials and cached entitlements |
| GET | `/api/account/plan` | — | Cached entitlement and stale marker |
| POST | `/api/account/plan/refresh` | — | Explicit focus refresh, throttled to once per minute |
| POST | `/api/account/upgrade` | `{plan}` | Open checkout in the system browser |
| POST | `/api/account/portal` | — | Open subscription management in the system browser |

<!-- revision: 3 -->
