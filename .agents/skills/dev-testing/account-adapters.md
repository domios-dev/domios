# Account, billing and analytics development adapters

These development settings were moved out of embedded ACCOUNT help. Follow
the dev-testing instance isolation rules before changing or exercising them.

`account.provider` selects `local` (without build-time Firebase configuration), `fake` (tests) or `firebase`. Local signs in `local-dev-user` without cloud setup. Provider configuration is read when the account service first starts; restart after changing it. Firebase defaults come from build-time environment variables: DOMIOS_FIREBASE_API_KEY, DOMIOS_FIREBASE_PROJECT_ID, DOMIOS_FIREBASE_APP_ID and, in DEV, DOMIOS_ACCOUNT_REDEEM_URL. Complete values select Firebase by default; missing values select local. Release sign-in is pinned to https://auth.domios.dev/signin/ and redemption to https://us-central1-domios-d9357.cloudfunctions.net/redeemDesktopLoginCode; release ignores endpoint config and endpoint build variables, while account.api_key still supplies the Firebase client API key. DEV supports account.signin_url and account.redeem_url overrides; an incomplete Firebase configuration falls back to local. DEV with the explicit DOMIOS_FIREBASE_EMULATOR=1 runtime opt-in selects the synthetic demo-domios emulator services instead (see the dev-run skill, "Firebase emulator DEV"); emulator selection is compiled out of release. No vendor key is stored in the repository. `account.login_timeout_secs` defaults to 180 (valid range 1–600). Login carries a random state and S256 challenge to the system browser. Both the loopback callback and `domios://auth/callback` require matching state; redemption submits the verifier to the backend. The refresh credential stays in the configured secret store, and the ID token stays in memory. A saved credential is refreshed at startup.

`billing.provider` is `cloud`; release pins its endpoint to `https://us-central1-domios-d9357.cloudfunctions.net/` and the PRODUCTION entitlement key. DEV `billing.endpoint` overrides build-time DOMIOS_BILLING_FUNCTIONS_URL and must be an HTTPS base URL ending in `/`, the selected Firebase emulators, or the loopback DEV issuer `http://127.0.0.1:<port>/`. The endpoint only chooses a backend: access comes solely from a verdict signed by the key the build pins, and anything else is compiled free.

## DEV entitlement QA (signed TEST verdicts)

For isolated cloud sign-in and sandbox purchases, build with
`DOMIOS_ACCOUNT_TARGET=qa ./build.sh dev` (also supported by `dev-back`).
The build secret store must contain `DOMIOS_QA_PROJECT_ID=domios-signin-qa`,
`DOMIOS_QA_FIREBASE_API_KEY`, and `DOMIOS_QA_FIREBASE_APP_ID`. These are separate
from the production secrets used by install-local. Expect the startup log
`account target: qa (domios-signin-qa)`. Sign in using the secret-store test
accounts `domios-qa-free` and `domios-qa-buyer`; the buyer exercises the sandbox
checkout. Never print their credentials. QA derives sign-in, redemption and
billing URLs from that project and rejects saved account or billing overrides
that select another target. Clear those overrides before starting QA. Missing
QA values or a simultaneous emulator opt-in refuse startup with an explicit
diagnostic. The target is compiled into debug builds; release and install-local
ignore it and retain production endpoints. Production build secrets stay unchanged.

### Agent sign-in without exposing the password

Use an Orchestrator-owned DEV built with `DOMIOS_ACCOUNT_TARGET=qa`; verify its
health SHA and QA target first. Workers do not start lane servers. In a DEV-bound
bash utility shell, start login and leave the request running until completion:

```bash
source evals/_lib/lib.sh
dc account login
```

In a second DEV-bound shell, read the pending URL and open the calling agent's
Browser on the same machine as DEV. The authenticated debug-only route returns
404 unless a hosted login is pending; the verifier remains in-process.

```bash
source evals/_lib/lib.sh
login_url=$(dev_get /api/account/login/pending | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["url"])')
domios plugin browser open "$login_url" --profile clean
domios plugin browser snapshot
```

Select email sign-in if the hosted page first offers provider choices, then use
the controls' exact refs from each fresh snapshot. The secret-store entry is
a JSON object with `email` and `password`. Run without shell tracing; never print
or save the secret. Substitute the observed refs in these commands:

```bash
domios mem secret get --name domios-qa-free | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["email"])' | domios plugin browser type <email-ref> --stdin
domios plugin browser snapshot
domios mem secret get --name domios-qa-free | python3 -c 'import json,sys; sys.stdout.write(json.load(sys.stdin)["password"])' | domios plugin browser type <password-ref> --stdin
domios plugin browser press Enter
```

The extraction writes exact bytes without a trailing newline. Stdin typing also
masks the email when using this procedure. Observe the hosted success page and
the first request completing, then use `dev_get /api/account/status` to prove the
QA identity is signed in and `pending` is false. The pending route must now return
404. A hosted success page alone does not prove DEV login completed. Never reveal
the password field or capture screenshots with unmasked credentials.

Debug builds pin the TEST key `test-2026-10` and take the TEST project from build-time `DOMIOS_QA_PROJECT_ID`, which build.sh injects from the secret store. Without it, DEV refuses every verdict and stays plain free. The signing key is the secret `entitlements/test-signing-key` (base64 PKCS#8 DER); never print or copy it.

1. Build DEV through build.sh on a machine whose secret store holds `DOMIOS_QA_PROJECT_ID` and `entitlements/test-signing-key`.
2. Run `node scripts/dev-entitlements-issuer.mjs smoke` to prove the stored TEST key signs a Sponsor verdict for `local-dev-user` and the QA project; the command verifies against the pinned TEST public key without printing secrets. Start the issuer: `node scripts/dev-entitlements-issuer.mjs serve --port 15200` (loopback only).
3. In DEV set `account.provider` to `local`, `billing.provider` to `cloud`, and `billing.endpoint` to `http://127.0.0.1:15200/`, restart DEV, and sign in as `local-dev-user`. Debug cloud billing sends a `domios-local-dev.` bearer containing the uid and installation id only to loopback HTTP `getEntitlements`; the issuer requires its installation id to match the request body. Other endpoints and billing actions retain the normal account token. Release compiles out this bearer path.
4. Switch the plan with `node scripts/dev-entitlements-issuer.mjs mode <mode>`, then refresh with `POST /api/account/plan/refresh` or app focus. Focus refreshes are throttled to one per 60 s.
   - `free`: signed unpaid verdict; free limits enforced.
   - `pro` / `sponsor`: signed paid verdict; no locks.
   - `expired`: a paid verdict whose 24 h lease already ended; refused, so free limits apply and items lock (never deleted).
   - `offline`: 503; a still-valid cached verdict holds until its lease ends, otherwise free.
5. Confirm the issuer logs `sponsor uid=local-dev-user`, and the refresh response reports Sponsor access with a verified grant. A server log alone does not prove the app accepted the signature. Canonical Rust selectors: `./build.sh test-one billing adapters::cloud::tests::`; the release-configuration test `release_local_session_has_no_dev_bearer` must also run with debug assertions disabled. Release pins the PRODUCTION key and endpoint, so TEST verdicts stay free.

Product analytics is off by default; Settings > Privacy or `domios analytics status|enable|disable` records opt-in consent in `analytics-settings.json`, and turning it off clears queued events and installation/account identifiers. `DOMIOS_ANALYTICS_MODE` selects `off` (DEV default), `console` (DEV debug logs, no external account) or `amplitude` (release default); `DOMIOS_AMPLITUDE_SERVER_ZONE` is `us` (default) or `eu`; `DOMIOS_ANALYTICS_PROJECT` is `test` (DEV default) or `prod` (release default); set them before starting the daemon. The Amplitude HTTP V2 key is a public ingestion key embedded at build time through DOMIOS_AMPLITUDE_API_KEY_PROD (or DOMIOS_AMPLITUDE_API_KEY_TEST); the secret store `domios.amplitude.prod.api_key` overrides it for local builds (`domios.amplitude.test.api_key` for test projects). Key availability is cached for the process; restart after changing the override. With no key, the adapter holds the queue and analytics status reports vendor_available=false. Every sink shares the consent gate, the closed product event catalog below, the persisted queue and the bounded retry policy.
