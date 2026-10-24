# Domios Account and Analytics

Sign in, manage a subscription, and control optional product analytics through the Account menu, Settings, or the CLI.

## Account

Provider configuration is read when the account service starts. `account.login_timeout_secs` defaults to 180 (valid range 1–600). Login opens the system browser with a random state and S256 challenge. Browser callbacks require matching state; redemption submits the verifier to the backend. Account refresh credentials and the Mesh installation credential stay in the local owner-only `<data_dir>/.secrets` file. They never use the configured secret backend or OS credential store. The ID token stays in memory. A saved credential is refreshed at startup. A one-time background migration attempts to move old credentials without prompting; slow or unavailable stores never delay startup. If the private file cannot be written, the sign-in session stays in memory only and does not survive restart.

```bash
domios account login
domios account status
domios account logout
```

Account status includes `last_sign_in_failure`: `null`, or a safe `{stage, error}` record for the last completed unsuccessful sign-in. Stages are `preparation`, `browser`, `callback`, `redemption`, `credential_save`, and `publication`; redemption includes provider token exchange and profile retrieval. Errors are closed account error categories, never provider response bodies, URLs, identifiers or credentials. The record stays available across status reads and client remounts, and clears when an explicit sign-in starts or a session succeeds. Account surfaces show a plain-language message from this record. It describes only attempts completed by this running app; it cannot diagnose a past attempt made before this instrumentation.

## Subscription

```bash
domios account plan
domios account upgrade pro
domios account portal
```

The round avatar and chevron immediately left of Settings open the Account menu. Signed-out users can choose Sign in, Create account, Google, Facebook or email. Sign-in and account creation share the system-browser flow; the mode changes its heading. When signed in, its initial opens identity, plan, Manage subscription and Sign out. Both account states offer Community links for Telegram and Discord; the Guide offers the same links, opened in the system browser. Settings > Account shows the same plan, status, period end, cancellation flag and stale marker. Upgrade and Manage subscription open the system browser. The client reads only the signed-in user's server record using the in-memory ID token. Signed-out users get free. A failed fetch retains the last known entitlement marked stale. Sign-in, explicit app focus (at most once per minute), and checkout return refresh the cache; browser return alone never grants access. Returning from checkout to the app bypasses the focus throttle once. Hosted completion may also open `domios://billing/return`.

The cloud service supplies subscription entitlements. Without a cloud endpoint, the app returns free and subscription actions report that cloud billing is unavailable.

### Locked items

When free-plan limits are enforced, creating one more workspace, Team or tab past the limit is refused with `free_limit_reached`. Items that already exist beyond the limit (for example after a downgrade) are never deleted or hidden: every listing still returns them. Instead the oldest items up to the limit stay usable and the newer ones are marked locked; the General workspace never counts and never locks. Acting on a locked item is refused with the stable code `plan_limit_locked`, naming the item and the limit. Workspace, tab and Team payloads carry `locked: true` on locked items; when the entitlement changes, Domios recomputes the marks and emits `entitlement-changed` without a restart. Order is fixed by each item's creation time, so reordering never moves a lock. Upgrading unlocks everything; closing older items frees their slots. Builds that do not enforce limits lock nothing. The app shows a lock badge on locked items, treats every tab inside a locked workspace as locked, and opens the plans dialog when one is clicked; its terminal and composer take no input. Opening a locked item from the command palette or keyboard lands on a view that already shows the lock overlay and locked composer.

## Product analytics

Product analytics is off by default. Settings > Privacy or `domios analytics status|enable|disable` records opt-in consent. Turning it off clears queued events and installation/account identifiers. If the analytics service is unavailable, the app holds the queue; analytics status reports `vendor_available=false`. Delivery follows the consent gate, the closed product event catalog below, and a bounded retry policy.

Every event contains its catalog properties plus a random installation ID (`device_id`), unique event ID (`insert_id`) and timestamp (`time`). An authenticated account contributes only its exact Firebase account UID (`user_id`, without hashing). Sign-out removes it from subsequent events; events already queued keep the identity that owned them. Sign-in while analytics is off does not persist an analytics account ID. Opting in while signed in attaches the current account ID. A restarted daemon clears its active analytics account ID until account refresh establishes the identity again.

Prompts, messages, terminal output, paths, workspace or tab names, email addresses, display names, session IDs, peer identities, credentials and arbitrary plugin model or provider names are never included. Custom providers become `other`; models are never collected. No emitter can bypass the consent gate: consent off prevents queueing and analytics network delivery, clears queued events and analytics identifiers, and a later opt-in creates a new random installation ID. Settings > Privacy > What we collect shows the same closed catalog.

Managed app updates report only their kind after the requested app version is running. Frontend updates report only their kind after enrolled clients prove health and activation is saved. Plan creation follows successful persistence. Pairing and workspace creation carry the saved post-change counts. Checkout follows a successful browser open; subscription changes follow authoritative entitlement refresh, with unchanged plan/status and failed refreshes suppressed.

`app_first_run` records the first consented startup with only version and platform; its saved marker is committed only after delivery and survives restarts and opt-out. An undelivered first-run event remains pending in the queue; if dropped, the next startup enqueues it again. `app_opened` accompanies each consented daemon startup. Enabling analytics during the current run records these startup events after consent, using the current signed-in UID when available. `plan_viewed` records a free-limit plans modal opening, an explicit subscription-plans request, or observed visibility of the plan cards, with no account or plan properties.

`active_day` records the first app start, user message admission, agent spawn or workspace switch in a local calendar day. `feature_used` records successful use of each listed feature at most once per local calendar day. Both gates persist alongside analytics settings, survive restarts, and use activity events without timers or polling. Opt-out clears the saved days together with identifiers and queued events. Machine counts include this machine plus paired peers; workspace counts include all workspaces.

## Product event catalog

Versions contain numeric major, minor and patch only, without prerelease labels or build metadata. Counts are unsigned 32-bit integers. Provider IDs come from the bundled registry; runtime free-form provider names become `other`. Events with no properties carry an empty object. Feature adoption includes completed dreaming runs, saved crons and Blueprints, saved theme changes, confirmed Telegram sends or accepted inbound messages, accepted voice input, installed plugins, formed Teams, saved memory and opening the web interface.

<!-- analytics-catalog:start -->
| Event | Properties |
|---|---|
| `app_first_run` | `version`: Version, `platform`: Platform |
| `app_opened` | None |
| `plan_viewed` | None |
| `app_started` | `version`: Version, `platform`: Platform, `mesh_machines`: u32, `workspaces`: u32 |
| `mesh_peer_paired` | `mesh_machines`: u32 |
| `workspace_created` | `workspaces`: u32 |
| `signed_in` | `auth_provider`: AuthProvider |
| `checkout_started` | `plan`: PlanKey |
| `subscription_changed` | `plan`: PlanKey, `status`: SubscriptionStatus |
| `analytics_opted_in` | None |
| `agent_spawned` | `provider_id`: ProviderId, `role`: Role |
| `update_installed` | `kind`: UpdateKind |
| `plan_created` | None |
| `active_day` | `mesh_machines`: u32, `workspaces`: u32 |
| `feature_used` | `feature`: Feature |

- **Platform:** `macos`, `windows`, `linux`
- **AuthProvider:** `local`, `fake`, `firebase`
- **PlanKey:** `free`, `pro`
- **SubscriptionStatus:** `active`, `past_due`, `canceled`, `revoked`, `incomplete`, `trialing`, `unpaid`, `paused`
- **Role:** `general`, `orchestrator`, `planner`, `worker`, `manager`, `supporter`, `none`, `other`
- **UpdateKind:** `app`, `frontend`
- **Feature:** `dreaming_run`, `cron_created`, `blueprint_created`, `theme_changed`, `telegram_bridge_used`, `voice_input_used`, `plugin_installed`, `team_created`, `memory_saved`, `web_ui_opened`
- **ProviderId:** `claude`, `codex`, `cursor`, `opencode`, `other`
<!-- analytics-catalog:end -->

## Local API

Local callers only.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/account/status` | — | Account identity, effective provider and pending flag |
| POST | `/api/account/login` | — | Browser sign-in; optional provider=google/facebook/email and mode=signin/signup query hints |
| POST | `/api/account/logout` | — | Block Mesh traffic, withdraw this machine's endpoint once, then clear credentials and cached entitlements; saved pairs and the machine slot stay |
| GET | `/api/account/plan` | — | Cached entitlement and stale marker |
| POST | `/api/account/plan/refresh` | — | Explicit refresh bypasses throttling, including checkout return; passive window focus uses ?focus=true, throttled to once per minute |
| POST | `/api/account/upgrade` | `{plan, theme?}` | Open checkout in the system browser; `theme` (`light` or `dark`) sets the checkout page's theme |
| POST | `/api/account/portal` | — | Open subscription management in the system browser |
| GET | `/api/account/mesh` | — | Account Mesh snapshot: `binding`, `traffic`, `cleanup`, `self`, last `registry` observation, pending `limit_notice` |
| POST | `/api/account/mesh/enroll` | — | Retry enrollment; a refusal returns `{ok:false, request_id, failure}` |
| POST | `/api/account/mesh/list` | — | Read the account's machines online into the snapshot |
| POST | `/api/account/mesh/rename` | `{expected_metadata_revision, display_name}` | Rename this machine |
| POST | `/api/account/mesh/session-revoke` | `{installation_id?: string | null}` | Revoke a machine's account sessions; omitted installation selects this machine; retains the installation key and pairings |
| POST | `/api/account/mesh/revoke` | `{target}` | Release a machine's account slot; local unpair stays `/api/mesh/peers/disconnect` |
| POST | `/api/account/mesh/limit-notice/ack` | `{request_id}` | Mark a `mesh.machines` notice as shown |

Mesh enrollment never fails sign-in. Only a `mesh.machines` refusal (`{reason, limit, active_count}`) opens the plans dialog; offline, rate, authentication and verification refusals do not. A machine refused that way reports `traffic: "plan_limit_locked"` in the snapshot: its Mesh edges stay blocked, its rows stay listed, and a plan change re-enrolls it live.

Authenticated server refusals with `UNAUTHENTICATED` and `details: {"reason":"session_revoked"}` clear the local account session, keep its installation key, block Mesh, and open sign-in with “This machine was signed out from your account”. A real sign-in restores access. My machines and Mesh ask for confirmation naming the selected machine. Remove from mesh offers an optional “Also sign it out” checkbox; the account menu offers Sign out this machine alongside normal Sign out.

<!-- revision: 17 -->
