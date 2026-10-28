# Product analytics event plan

This is the event inventory for Domios desktop, codeterm-firebase and codeterm-web. Desktop crate paths below are relative to `src-tauri/crates/`; frontend paths start with `src/`. Backend and marketing paths are relative to their respective repositories.

Only consenting users contribute analytics. Event properties are closed enums, bounded numeric versions and counts. No prompts, message bodies, terminal output, paths, file names, email addresses, friendship IDs or arbitrary text may enter event properties. Identity IDs are separate envelope fields. Website `path` is a finite public-route enum, not an arbitrary URL; queries and fragments are discarded. Referrers and campaign parameters are classified into closed categories, never forwarded verbatim.

## Consent and identity

Desktop `service/analytics/bridge.rs::set_product_analytics_consent` first applies the local choice. For signed-in Firebase accounts, explicit ON reads the current backend revision and submits `analyticsConsent` with that revision and a fresh change ID; OFF submits an unconditional OFF. A stale/refused update surfaces as a settings error, without rebasing/retrying an ON intent. A successful ON calls `analyticsSignIn` with the worker's exact device ID. `account.rs::account` restore and `finish_account_login` also link a locally consenting installation. Those automatic links never override a recorded server OFF or erasure fence. The hosted browser bootstrap has the same rule.

The root cause was absent desktop calls to either callable: local opt-in only reached `Worker::set_consent`; `AnalyticsIdentity::changed` only set the local UID. Backend callable, authenticated consent persistence and identity queueing already existed. The worker already retained the account UID while local consent was off.

- Desktop envelope `user_id` is the exact Firebase UID while signed in; sign-out removes it from later events. `device_id` is the installation-scoped analytics UUID v4 returned by `Worker::device_id`, created on opt-in and rotated after opt-out. It is distinct from the account enrollment installation ID. Startup and pre-sign-in events share this device ID.
- Backend `analyticsSignIn` authenticates the bearer token, checks session revocation, and queues `$identify` with that same `user_id` and `device_id`. Business/payment events use the Firebase UID; they do not guess one of an account's devices. This identity event joins the desktop's anonymous history to the account in the same Amplitude project.
- Web `domios_did` is a random UUID v4 in a Secure, SameSite=Lax, first-party cookie on `.domios.dev`. It is created only after website consent. It is the web event's envelope `device_id`, including download events; it is not duplicated as a property.
- `hosting/signin/analytics.ts::createSignInAnalytics`, called by `hosting/signin/adapters/firebase.ts` after browser authentication, reads that consented cookie and calls `analyticsSignIn` once per hosted sign-in client. `$identify` links web device ID to the authenticated Firebase UID. Desktop later links its own device ID to the same UID. No email or other PII is sent as analytics properties.
- Different browsers/profiles, cleared/expired cookies, denied consent, blocked ingestion, or different Firebase accounts break the web join. An installer carries no attribution token. A download does not prove installation. `app_first_run` is the first consented startup, not an installer-completion event. Declined/pre-consent visits and feature actions are intentionally absent.
- Offline backend consent updates fail visibly; local OFF still takes effect immediately. No automatic ON rebase or background timer retry exists. If a server OFF cannot be delivered, retry the explicit OFF when online. Native emulator transport for the desktop bridge is not supported by the HTTPS-only HttpCallable; protocol tests inject a transport, and backend tests exercise localhost callables separately.

## Desktop events

Every row has source **desktop**.

| Event | Trigger point (file · function) | Trigger | Properties | Funnel step |
|---|---|---|---|---|
| `app_first_run` | `codeterm-analytics/src/engine.rs · Engine::enqueue_on_day` | First consented startup; only once after successful delivery | `version`: Version, `platform`: Platform | Activation |
| `app_opened` | `codeterm-analytics/src/engine.rs · Engine::enqueue_on_day` | Derived from app_started | None | Retention |
| `plan_viewed` | `src/components/billing/usePlanViewed.ts · usePlanViewed` | Subscription plan cards requested or observed | None | Purchase intent |
| `app_started` | `codeterm-service-core/src/service/analytics.rs · analytics_worker / app_started` | Daemon startup replayed when consent becomes on | `version`: Version, `platform`: Platform, `mesh_machines`: u32, `workspaces`: u32 | Activation |
| `mesh_peer_paired` | `codeterm-service-core/src/service/analytics.rs · mesh_observer` | Pairing accepted | `mesh_machines`: u32 | Adoption |
| `workspace_created` | `codeterm-service-core/src/service/workspace.rs · create_workspace` | Workspace creation committed | `workspaces`: u32 | Adoption |
| `signed_in` | `codeterm-service-core/src/service/account.rs · AnalyticsIdentity::changed` | Account identity accepted | `auth_provider`: AuthProvider | Sign-in |
| `checkout_started` | `codeterm-billing/src/cache.rs · checkout_browser_opened → service/analytics.rs · billing_observer` | Checkout browser launch succeeded | `plan`: PlanKey | Checkout |
| `subscription_changed` | `codeterm-billing/src/cache.rs · finish_refresh → service/analytics.rs · subscription_event` | Verified subscription projection changes | `plan`: PlanKey, `status`: SubscriptionStatus | Conversion/retention |
| `analytics_opted_in` | `codeterm-analytics/src/worker.rs · run / Command::Consent` | Off → on consent change | None | Consent |
| `agent_spawned` | `codeterm-service-core/src/service/analytics.rs · record_product_agent_spawned` | Agent creation accepted | `provider_id`: ProviderId, `role`: Role | Adoption |
| `update_installed` | `codeterm-service-core/src/service/analytics/update_confirm.rs · confirm` | Confirmed updater receipt | `kind`: UpdateKind | Retention |
| `plan_created` | `codeterm-service-core/src/service/analytics.rs · analytics_worker plan observer` | Plan store creates a plan | None | Adoption |
| `active_day` | `codeterm-analytics/src/worker.rs · record_activity_on_day` | First product activity of a local day | `mesh_machines`: u32, `workspaces`: u32 | Retention |
| `feature_used` | `codeterm-service-core/src/service/analytics.rs · record_product_feature; src/lib/productAnalytics.ts · emitFeatureUsed` | Bounded long-tail feature action (table below) | `feature`: Feature | Adoption |
| `limit_reached_shown` | `src/components/billing/freeLimitGate.ts · setPrompt` | New resource limit prompt published | `resource`: Resource | Limit |
| `upgrade_clicked` | `src/components/billing/freeLimitGate.ts · upgradePlan` | Upgrade action begins, before checkout request | `source`: UpgradeSource | Upgrade |
| `checkout_opened` | `codeterm-service-core/src/service/analytics.rs · billing_observer` | Successful checkout browser launch | `outcome`: CheckoutOutcome | Checkout |
| `checkout_returned` | `codeterm-billing/src/cache.rs · begin_refresh → service/analytics.rs · billing_observer` | First focus/explicit return refresh after a checkout launch; portal excluded | `outcome`: CheckoutOutcome | Return |
| `friend_added` | `src/components/Neighbors/neighborClient.ts · apply` | Incoming friendship acceptance succeeds | `count`: u32 | Adoption |
| `neighbor_message_sent` | `src/components/Neighbors/neighborRuntimeClient.ts · neighborRuntimeClient.send; neighborFilesClient.ts · send` | Confirmed relay submission; excludes refused/unconfirmed submission | `count`: u32 | Adoption |
| `file_shared` | `src/components/Neighbors/neighborFilesClient.ts · send` | Confirmed file-message submission | `count`: u32 | Adoption |
| `read_receipt_shown` | `src/components/Neighbors/useReadReceiptAnalytics.ts · useReadReceiptAnalytics` | Read indicator intersects visible viewport in a visible document, once per mounted indicator | `count`: u32 | Adoption |
| `settings_viewed` | `src/hooks/useSettingsNavigation.ts · useSettingsNavigation` | Settings route section changes | `section`: SettingsSection | Adoption |

`checkout_returned.outcome=returned` proves the application regained focus following a checkout launch. It does not claim payment success. Only the backend verified-payment event is authoritative for conversion. `subscription_changed` exists in both sources: filter by envelope/available properties when inspecting it; never add desktop and backend counts as unique purchases. `friend_added` measures acceptance on this device; the requesting person's later remote acceptance is not a second local button event. File counts are selected attachment counts following relay acceptance, not proof that the recipient saved them.

### Long-tail feature enum

These are values of `feature_used.feature`, not separate event names. Each value is emitted at most once per local calendar day. The dispatch brief's older standalone long-tail event list does not match the current catalog.

| Feature | Trigger point | Source | Funnel step |
|---|---|---|---|
| `dreaming_run` | `codeterm-api/src/mem_routes/dream.rs` · mem_dream | desktop | Adoption |
| `cron_created` | `codeterm-service-core/src/service/cron.rs` · cron_create | desktop | Adoption |
| `blueprint_created` | `codeterm-sentinel-host/src/blueprint_apply.rs` · apply_document | desktop | Adoption |
| `theme_changed` | `codeterm-api/src/config_routes.rs` · patch_config_api | desktop | Adoption |
| `telegram_bridge_used` | `codeterm-service-core/src/bridge_host.rs` · ServiceBridgeTabPort::telegram_used | desktop | Adoption |
| `voice_input_used` | `codeterm-service-core/src/service/local_recording/admission.rs` · local_recording_commit | desktop | Adoption |
| `plugin_installed` | `codeterm-commands/src/commands/plugins/action.rs` · run_action | desktop | Adoption |
| `team_created` | `codeterm-orchestration/src/orchestration_pair.rs`, `orchestrator_onboard.rs` · orchestration_pair / deliver_orchestrator_onboarding | desktop | Adoption |
| `memory_saved` | `codeterm-api/src/mem_routes/items/mod.rs` · mem_save | desktop | Adoption |
| `web_ui_opened` | `src/App.tsx` · web-client initialization effect | desktop service via web client | Adoption |

Desktop enum definitions and count bounds are authoritative in `src-tauri/crates/codeterm-analytics/catalog.schema.json`. Generated Rust, TypeScript and the in-app privacy catalog come from `node scripts/check-analytics-catalog.mjs --write`. The plan enum currently maps paid desktop plans to `pro`; Sponsor segmentation is unavailable in this catalog.

## Backend events

Every row below has source **backend**. Production business emitters currently use empty event properties unless stated. The contract permits additional reviewed dimensions, but an allowlist is not evidence of an emitter. All delivery is fenced by current server consent and erasure state; business transactions stage analytics atomically via `analytics/atomic.ts::prepareProductEvent`.

| Event | Trigger point (functions/src unless prefixed shared/) | Emitted properties | Funnel step/status |
|---|---|---|---|
| `$identify` | `analytics/product.ts` · signInAnalytics, exposed by `analytics/firebase.ts::analyticsSignIn` | None; UID and device UUID in envelope | Identity join |
| `sign_in_completed` | `analytics/product.ts` · signInAnalytics | None | Hosted/desktop identity bridge |
| `checkout_created` | `billing/index.ts` · callable checkout participant using prepareProductEvent | None | Checkout created by backend |
| `subscription_started` | `billing/adapters/firestore.ts` · FirestoreBillingStore.apply | None | Initial subscription projection |
| `subscription_changed` | `billing/adapters/firestore.ts` · FirestoreBillingStore.apply | None | Subscription projection changes |
| `subscription_canceled` | `billing/adapters/firestore.ts` · FirestoreBillingStore.apply | None | Cancellation |
| `first_verified_payment` | `billing/trial-admission.ts` · prepareTrialPublication; `billing/adapters/firestore.ts` · createFirestoreTrialJournal / createFirestoreTrialWebhookHandler | None | Authoritative first positive verified payment |
| `trial_started` | Same committed trial publication path | None | Authoritative trial start |
| `first_charge_failed` | Same committed trial publication path | None | First charge failure |
| `renewal` | Same committed trial publication path | None | Verified paid renewal |
| `churn` | Same committed trial publication path | None | Committed churn transition |
| `gift_redeemed` | `gift-keys/store.ts` · createGiftKeyStore stage callback | `plan`: paid-plan enum | Gift conversion |
| `referral_signup` | `referral-trial/service.ts` · createReferralTrialService / decide | None | Referral trial admission |
| `referral_rewarded` | `referral-trial/reward.ts` · createReferralRewardApplier | None | Referral reward |
| `limit_reached_shown` | `shared/analytics-contract.ts::ANALYTICS_KINDS`, `analytics/events.ts::PROPERTY_KEYS` | Contract: plan, app_version, platform, resource | Reserved backend contract; desktop emitter above |
| `upgrade_clicked` | Same contract/allowlist | Contract: plan, app_version, platform, source | Reserved backend contract; desktop emitter above |
| `checkout_opened` | Same contract/allowlist | Contract: common dimensions | Reserved backend contract; desktop emitter above |
| `checkout_returned` | Same contract/allowlist | Contract: common dimensions, outcome (completed/cancelled/unknown) | Reserved backend contract; desktop returned outcome differs |
| `referral_link_shared` | Same contract/allowlist | Contract: plan, app_version, platform, referral_id | Reserved; no production emitter found |
| `referral_reward_revoked` | Same contract/allowlist | Contract: plan, app_version, platform, referral_id | Reserved; no production emitter found |

Backend common dimensions: plan, catalogued price_id/discount_id, bounded trial_length_days, via_trial boolean, catalogued app_version/platform. Provider-transition contracts additionally allow catalogued country. `analytics/events.ts::properties` rejects arbitrary values and unknown keys. `PRODUCT_CATALOGUE` currently has empty optional dimension lists, so do not assume live segmentation by those dimensions. Business provider IDs and journal evidence are not sent as arbitrary properties.

`analytics/firebase.ts::deliverAnalytics` handles new analyticsDue rows; retryAnalytics handles bounded retries. `analytics/vendor.ts::createVendor` reconstructs and validates the outbound envelope. No keys or live Amplitude queries are required for local verification.

## Marketing website events

Every row has source **web**, from codeterm-web. `analytics/controller.ts::createAnalytics` loads the existing configured browser SDK only after consent and validates every event again at its transport. It disables autocapture, SDK identity persistence, remote config and enrichment; strips PII fields; sends no referrer header, cookies or IP-derived properties to ingestion. Withdrawal aborts requests, opts out and removes the visitor cookie.

| Event | Trigger point | Properties | Funnel step |
|---|---|---|---|
| `page_viewed` | `src/analytics/controller.ts` · page, via ConsentBanner; browser.ts supplies attribution | path: /, /checkout variants, /billing/return variants; referrer/utm_source/utm_medium/utm_campaign closed enums below | Visit |
| `video_started` | `src/analytics/useFilmAnalytics.ts` · film subscription reports playing | None | Engagement |
| `video_progress` | Same hook · numeric Seek control value changes, or ended state | quartile: 25/50/75/100; seconds_watched: 0/15/30/60/120/300 lower-bound buckets | Engagement |
| `download_clicked` | `src/download/useDownload.ts` · createDownloadFlow change callback after download lookup resolves | platform: mac/windows/linux; arch: arm64/x64; validated numeric release version or null; device_id in envelope | Download initiated |
| `pricing_viewed` | `src/checkout/CheckoutPage.tsx` · Compare intersection; optional landing #pricing intersection in useLandingAnalytics | None | Pricing viewed (landing currently has no pricing section) |
| `cta_clicked` | `src/analytics/useLandingAnalytics.ts` · delegated anchor click | cta: download/github/explore/pricing/discord/telegram | Intent |
| `scroll_depth` | Same hook · passive scroll handler | depth: 25/50/75/100 | Engagement |
| `checkout_viewed` | `src/analytics/controller.ts` · page for checkout routes | None | Checkout page |
| `checkout_completed` | `src/checkout/CheckoutPage.tsx` · checkout callback | plan: pro | Client-reported checkout completion; not verified payment |

Web attribution enums: referrer = direct/domios.dev/google.com/github.com/youtube.com/other; utm_source = direct/google/github/youtube/discord/telegram/other; utm_medium = none/organic/social/email/cpc/referral/other; utm_campaign = none/launch/other. Unrecognized values become other; no arbitrary domains or campaign text escape. Video progress uses the film's accessible numeric Seek control because its current subscription omits numeric time; the hook stops observing the whole film once that control appears. Only positive playing deltas below two seconds accumulate watched time; seeks do not inflate watched seconds. The vendored film renders its Seek control inside an open shadow root while playing, including responsive layouts; completion is read from the subscription when controls unmount. A future numeric FilmState contract would remove this adapter dependency.

## Funnels for the owner to configure

1. Web acquisition: page_viewed (path=/) → download_clicked. Break down by platform, arch and bounded attribution categories. Inspect video_progress and scroll_depth as engagement segments.
2. Joined acquisition, for consented users with a shared hosted-auth browser: page_viewed → download_clicked → app_first_run → signed_in → feature_used or a key Neighbor event → limit_reached_shown → upgrade_clicked → checkout_started or checkout_opened → checkout_returned → first_verified_payment or trial_started. A person who opts in after signing in may lack an earlier signed_in event. Use sign_in_completed as the identity-bridge alternative, and allow optional steps rather than treating missing pre-consent events as drop-offs.
3. Payment retention: first_verified_payment → renewal; analyze churn separately. Client checkout_completed and desktop checkout_returned are intent/return evidence, never payment or trial authority.

Use Amplitude's identity merge on the UID, not device equality across browsers/installations. Use ordered steps with a conversion window appropriate to download/install delays, and compare unique users rather than raw event sums. The two checkout-launch events describe one action; configure an OR step rather than counting them as two required actions.

## Local proof and live readback

Local checks cover catalog regeneration, typed property rejection, worker consent/UID/device payloads, injected callable ordering, causal backend consent and erasure, queued identity envelopes, synthetic localhost callable persistence and web SDK transport stripping. Rust tests run only through the canonical owner's authorized gate. Test fixtures use synthetic identities and transport credentials; no Amplitude key lookup or live ingestion query is part of verification.

After reviewed deployments, the owner should use the Amplitude UI to confirm a consenting test user's page_viewed/download_clicked under the browser device ID, `$identify` linking that ID to the Firebase UID, a second `$identify` linking the desktop analytics device ID to that same UID, and verified backend payment/trial events on that user. Confirm no events before consent or after confirmed withdrawal, and inspect event properties for only the documented enums/counts. Check US project/environment alignment, anonymous history merge, one checkout-return per launch, and no duplicate verified payments on webhook replay. Repeat once with declined consent and once with a cleared browser cookie to confirm the intended privacy and attribution limits.
