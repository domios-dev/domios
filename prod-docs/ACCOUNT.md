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

Product analytics is off by default; Settings > Privacy or `codeterm analytics status|enable|disable` records opt-in consent in `analytics-settings.json`, and turning it off clears queued events and installation/account identifiers. `DOMIOS_ANALYTICS_MODE` selects `off` (default), `console` (DEV debug logs, no external account) or `amplitude`; `DOMIOS_AMPLITUDE_SERVER_ZONE` is `us` (default) or `eu`; `DOMIOS_ANALYTICS_PROJECT` is `test` (default) or `prod`; set them before starting the daemon. The Amplitude adapter reads `domios.amplitude.test.api_key` or `domios.amplitude.prod.api_key` from the secret store and holds the queue while it is missing. Every sink shares the consent gate, the closed product event catalog below, the persisted queue and the bounded retry policy.

Every event contains its catalog properties plus a random installation ID (`device_id`), unique event ID (`insert_id`) and timestamp (`time`). An authenticated account contributes only its opaque account ID (`user_id`). Sign-out removes it from subsequent events; events already queued keep the identity that owned them. Sign-in while analytics is off does not persist an analytics account ID. Opting in while signed in attaches the current account ID. A restarted daemon clears its active analytics account ID until account refresh establishes the identity again.

Prompts, messages, terminal output, paths, workspace or tab names, email addresses, display names, session IDs, peer identities, credentials and arbitrary plugin model or provider names are never included. Custom providers become `other`; unlisted models become `unknown`. No emitter can bypass the consent gate: consent off prevents queueing and analytics network delivery, clears queued events and analytics identifiers, and a later opt-in creates a new random installation ID. Settings > Privacy > What we collect shows the same closed catalog.

## Product event catalog

Versions contain numeric major, minor and patch only, without prerelease labels or build metadata. Counts are unsigned 32-bit integers. Provider and model categories come from the bundled provider registry; no runtime free-form model or provider value is sent. Shell tabs use a null provider. Events with no properties carry an empty object.

<!-- analytics-catalog:start -->
| Event | Properties |
|---|---|
| `app_started` | `version`: Version, `platform`: Platform, `update_channel`: UpdateChannel |
| `update_installed` | `from_version`: Version, `to_version`: Version, `kind`: UpdateKind, `attempt_count`: u32 |
| `workspace_created` | None |
| `tab_opened` | `kind`: TabKind, `provider_id`: Option&lt;ProviderId&gt; |
| `agent_spawned` | `provider_id`: ProviderId, `role`: Role, `model_family`: ModelFamily |
| `agent_restored` | `provider_id`: ProviderId, `outcome`: RestoreOutcome |
| `plan_created` | None |
| `task_dispatched` | `execution_intent`: ExecutionIntent |
| `mesh_peer_paired` | None |
| `signed_in` | `auth_provider`: AuthProvider |
| `checkout_started` | `plan_key`: PlanKey |
| `subscription_changed` | `plan`: PlanKey, `status`: SubscriptionStatus |
| `analytics_opted_in` | None |

- **Platform:** `macos`, `windows`, `linux`
- **UpdateChannel:** `stable`, `rc`, `nightly`
- **UpdateKind:** `app`, `frontend`
- **TabKind:** `shell`, `agent`
- **Role:** `general`, `orchestrator`, `planner`, `worker`, `manager`, `supporter`, `none`, `other`
- **RestoreOutcome:** `resumed`, `fresh_launch`, `failed`
- **ExecutionIntent:** `read_only`, `mutating`
- **AuthProvider:** `local`, `fake`, `firebase`
- **PlanKey:** `free`, `pro`
- **SubscriptionStatus:** `active`, `past_due`, `canceled`, `revoked`, `incomplete`, `trialing`, `unpaid`, `paused`
- **ProviderId:** `claude`, `codex`, `cursor`, `opencode`, `other`
- **ModelFamily:** `anthropic/claude-opus-4-8`, `anthropic/claude-sonnet-4-6`, `auto`, `best`, `claude-4.6-sonnet-medium`, `claude-opus-4-8-thinking-high`, `claude-opus-4-8-thinking-low`, `composer-2.5`, `fable`, `gemini-3.1-pro`, `gpt-5.2-codex`, `gpt-5.5`, `gpt-5.5-high`, `gpt-5.6-luna`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-6-astra`, `gpt-6-luna`, `gpt-6-sol`, `gpt-6.1-sol`, `grok-4.3`, `haiku`, `kimi-k2.5`, `opencode/big-pickle`, `opencode/deepseek-v4-flash-free`, `opencode/gpt-5.4-mini`, `opencode/gpt-5.5`, `opus`, `opusplan`, `sonnet`, `unknown`
<!-- analytics-catalog:end -->

<!-- revision: 2 -->
