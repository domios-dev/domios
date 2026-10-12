# Domios Account and Analytics

The account and analytics features each own a provider trait; vendors are adapters chosen by config, and each feature ships `local` and `fake` adapters that work with no vendor account.

## Account

`account.provider` selects `local` (default), `fake` (tests) or `firebase`. Local signs in `local-dev-user` without cloud setup. Provider configuration is read when the account service first starts; restart after changing it. With `firebase`, `account.signin_url` defaults to `https://domios-d9357.web.app/signin/`. `account.redeem_url` defaults to the Functions v2 HTTPS function URL pattern `https://us-central1-domios-d9357.cloudfunctions.net/redeemDesktopLoginCode`; set it to the URL reported by deployment. `account.api_key` defaults to the public web API key of project `domios-d9357`. `account.login_timeout_secs` defaults to 180 (valid range 1–600). Login carries a random state and S256 challenge to the system browser. Both the loopback callback and `domios://auth/callback` require matching state; redemption submits the verifier to the backend. The refresh credential stays in the configured secret store, and the ID token stays in memory. A saved credential is refreshed at startup.

```bash
codeterm account login
codeterm account status
codeterm account logout
```

## Product analytics

Product analytics is off by default; Settings > Privacy or `codeterm analytics status|enable|disable` records opt-in consent in `analytics-settings.json`, and turning it off clears queued events and installation/account identifiers. `DOMIOS_ANALYTICS_MODE` selects `off` (default), `console` (DEV debug logs, no external account) or `amplitude`; `DOMIOS_AMPLITUDE_SERVER_ZONE` is `us` (default) or `eu`; `DOMIOS_ANALYTICS_PROJECT` is `test` (default) or `prod`; set them before starting the daemon. The Amplitude adapter reads `domios.amplitude.test.api_key` or `domios.amplitude.prod.api_key` from the secret store and holds the queue while it is missing. Every sink shares the consent gate, the closed catalog (feature enums, counts, durations, booleans, numeric versions; never paths, prompts, names or terminal output), the persisted queue and the bounded retry policy.

<!-- revision: 1 -->
