use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use zeroize::Zeroizing;

use crate::config::schema::AppConfig;
use crate::mem::{MemKind, MemManager, MemScope, NewMemItem, WakeUpOptions};
use crate::service::cloud_machines::ports::{
    Clock, CloudEnrollmentPort, CloudPeerControlPort, CloudProvisioningPort, CloudRouteSpec,
    EnrollmentBinding, EnrollmentOutcome, ProvisionedLease, ProvisionedMachine,
    ProvisioningCreateRequest,
};
use crate::service::cloud_machines::{
    CloudEndpoint, CloudError, CloudMachineController, CloudMachineRecord, CloudMachineStore,
    CloudRate, CloudSleepMode, CloudUsageSummary, CreateCloudMachineParams, DrainToken,
    FlushWriteOutcome, HeldCloudSend, PeerBusySnapshot, CLOUD_INGRESS_PORT,
};
use crate::service::general_agent::instructions::render_wake_up_section;

use super::apply::{
    apply_portable_preferences_to_config, apply_warm_bootstrap, read_verified_recovery_revision,
    TargetProvenance, WarmBootstrapApplyEnv, WarmBootstrapStateStore,
};
use super::export::{build_owner_projection, HomeExportInput};
use super::filter::{contains_forbidden_path, contains_secret_shaped_content, valid_portable_slug};
use super::providers::{classify_provider_capability, ProviderCapabilityProbe};
use super::types::{
    BackendCreatePhases, CloudCreatePhases, ExclusionCategory, ExclusionReason, PortableMemEntry,
    PortableMemKind, PortableOwnerIdentity, PortablePreferences, PortableRegisteredSkill,
    PortableSkillSource, ProviderSignInCardStatus, ProviderSignInState, WarmBootstrapApplyResult,
    WarmBootstrapPhaseEvent, WarmBootstrapProjection, WarmBootstrapRejectReason,
    WarmBootstrapStatus, WARM_BOOTSTRAP_SCHEMA_VERSION,
};

struct StepClock(AtomicU64);

impl StepClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    fn advance(&self, delta_ms: u64) {
        self.0.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl Clock for StepClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn export_and_apply_seeds_l0_l1_rule_skill_and_portable_prefs_while_excluding_secrets() {
    let home_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let home_mem = MemManager::open_in_memory().unwrap();
    home_mem.seed_bundled_if_needed().unwrap();

    std::fs::write(
        home_dir.path().join("identity.md"),
        "Owner Canary Name: Ada Lovelace (London)",
    )
    .unwrap();
    std::fs::write(home_dir.path().join("soul.md"), "Concise and direct.").unwrap();

    home_mem
        .save_replace(NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "L1 Canary Fact".into(),
            summary: Some("Primary canary codename is AURORA-42".into()),
            area: Some("profile".into()),
            tags: vec!["canary".into()],
            body: "Primary canary codename is AURORA-42".into(),
            source: Some("user".into()),
            importance: 9.0,
            source_path: None,
            pinned: true,
        })
        .unwrap();
    home_mem
        .save_replace(NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "Harmless Owner Rule".into(),
            summary: Some("Always prefix weekly summaries with [WEEKLY]".into()),
            area: Some("workflow".into()),
            tags: vec!["owner-rule".into()],
            body: "Always prefix weekly summaries with [WEEKLY]".into(),
            source: Some("user".into()),
            importance: 9.5,
            source_path: None,
            pinned: true,
        })
        .unwrap();
    home_mem
        .save_replace(NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "release-checklist".into(),
            summary: Some("Verified release checklist workflow".into()),
            area: Some("ops".into()),
            tags: vec!["version:1.2.0".into(), "ops".into()],
            body: "Run preflight, inspect SHA, verify artifact hashes.".into(),
            source: Some("user".into()),
            importance: 8.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    // Secret-shaped fixture: must be excluded and never appear in output.
    let secret_canary = "sk-ant-api03-SUPERSECRETKEYVALUE1234567890";
    home_mem
        .save_replace(NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "Leaked Key Fixture".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: format!("Do not copy: {secret_canary}"),
            source: Some("user".into()),
            importance: 9.0,
            source_path: None,
            pinned: true,
        })
        .unwrap();
    // Provider-home path fixture: must be excluded.
    home_mem
        .save_replace(NewMemItem {
            kind: MemKind::Preference,
            scope: MemScope::Global,
            title: "Provider Home Ref".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "Credentials live in ~/.provider-alpha/credentials.json".into(),
            source: Some("user".into()),
            importance: 8.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();

    let mut home_cfg = AppConfig::default();
    home_cfg.general_agent.agent_provider = "provider-beta".into();
    home_cfg.memory.l1_budget_tokens = 450;
    home_cfg.mesh.cloud_idle_sleep_minutes = 25;
    home_cfg.mesh.instance_name = "HomeDesktop".into();
    home_cfg.api.token = Some("home-only-bearer-token-999999".into());
    home_cfg.account = serde_json::json!({ "uid": "acct_home_secret" });

    let (projection, export_exclusions) = build_owner_projection(&HomeExportInput {
        account_id: "acct-owner-1",
        owner_installation_id: "inst-home-1",
        target_installation_id: "inst-cloud-1",
        creation_generation: "cloud-1:req-1",
        recovery_revision: None,
        mem: &home_mem,
        general_dir: home_dir.path(),
        config: &home_cfg,
        acp_owner_profiles: &BTreeMap::new(),
        available_provider_ids: &["provider-alpha".into(), "provider-beta".into()],
    });

    assert!(export_exclusions.iter().any(|e| {
        e.category == ExclusionCategory::WakeUpItem
            && e.reason == ExclusionReason::SecretShapedContent
    }));
    assert!(export_exclusions.iter().any(|e| {
        e.category == ExclusionCategory::WakeUpItem && e.reason == ExclusionReason::ForbiddenPath
    }));
    let serialized_proj = serde_json::to_string(&projection).unwrap();
    assert!(!serialized_proj.contains(secret_canary));
    assert!(!serialized_proj.contains("home-only-bearer-token-999999"));
    assert!(!serialized_proj.contains("HomeDesktop"));
    assert!(!serialized_proj.contains("~/.provider-alpha"));

    let target_mem = Arc::new(MemManager::open_in_memory().unwrap());
    target_mem.seed_bundled_if_needed().unwrap();
    let state_store = WarmBootstrapStateStore::in_data_dir(target_dir.path());
    let target_general = target_dir.path().join("general");
    let mut target_cfg = AppConfig::default();
    target_cfg.mesh.instance_name = "CloudTargetOwned".into();
    target_cfg.api.token = Some("target-owned-token".into());
    target_cfg.neighbors.key_store =
        Some(codeterm_types::config::neighbors::NeighborKeyStore::File);

    let env = WarmBootstrapApplyEnv {
        mem: &target_mem,
        general_dir: &target_general,
        state_store: &state_store,
        now_ms: 10_000,
        general_tab_id: Some("tab-main".into()),
        providers: vec![],
    };
    let provenance = TargetProvenance {
        enrolled_account_id: "acct-owner-1",
        target_installation_id: "inst-cloud-1",
        authenticated_caller_installation_id: "inst-home-1",
        verified_recovery_revision: None,
    };

    let result = apply_warm_bootstrap(
        projection,
        &provenance,
        &env,
        |prefs| Ok(apply_portable_preferences_to_config(&mut target_cfg, prefs)),
        || Ok(()),
    );

    assert_eq!(result.status, WarmBootstrapStatus::Applied);
    assert!(result.readable_owner_context);
    assert!(result.wake_up_verified);
    assert!(result.warm_console_ready_at_ms.is_some());
    assert_eq!(result.applied_counts.rules, 1);
    assert_eq!(result.applied_counts.skills, 1);

    // Verify normal General wake-up consumer renders seeded L0 identity, L1 canary, and owner rule.
    let wake_up = render_wake_up_section(Some(&target_mem));
    assert!(wake_up.contains("Ada Lovelace"));
    assert!(wake_up.contains("AURORA-42"));
    assert!(wake_up.contains("[WEEKLY]"));
    assert!(!wake_up.contains(secret_canary));

    // Verify registered skill & version resolved on target.
    let skill_items = target_mem
        .list_by_source_prefix("skill", "warm-seed:skill:")
        .unwrap();
    assert_eq!(skill_items.len(), 1);
    assert_eq!(skill_items[0].title, "release-checklist");
    assert!(skill_items[0].tags.iter().any(|t| t == "version:1.2.0"));

    // Verify portable preferences applied while target-owned settings stayed intact.
    assert_eq!(target_cfg.memory.l1_budget_tokens, 450);
    assert_eq!(target_cfg.mesh.cloud_idle_sleep_minutes, 25);
    assert_eq!(target_cfg.mesh.instance_name, "CloudTargetOwned");
    assert_eq!(target_cfg.api.token.as_deref(), Some("target-owned-token"));
    assert_eq!(
        target_cfg.neighbors.key_store,
        Some(codeterm_types::config::neighbors::NeighborKeyStore::File)
    );
}

#[test]
fn wrong_account_stale_generation_retry_local_edit_and_recovery_revision_guards() {
    let target_dir = tempfile::tempdir().unwrap();
    let target_mem = MemManager::open_in_memory().unwrap();
    target_mem.seed_bundled_if_needed().unwrap();
    let state_store = WarmBootstrapStateStore::in_data_dir(target_dir.path());
    let target_general = target_dir.path().join("general");

    let base_projection = |account: &str, generation: &str| WarmBootstrapProjection {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        account_id: account.into(),
        owner_installation_id: "inst-home".into(),
        target_installation_id: "inst-cloud".into(),
        creation_generation: generation.into(),
        recovery_revision: None,
        identity: Some(PortableOwnerIdentity {
            identity_md: Some("Owner: Grace Hopper".into()),
            soul_md: None,
            nickname: None,
        }),
        wake_up_items: vec![PortableMemEntry {
            kind: PortableMemKind::Rule,
            title: "Rule One".into(),
            summary: Some("Keep diffs small".into()),
            area: None,
            tags: vec![],
            body: "Keep diffs small".into(),
            importance: 8.0,
            pinned: true,
        }],
        skills: vec![],
        preferences: PortablePreferences::default(),
    };

    let provenance = TargetProvenance {
        enrolled_account_id: "acct-real",
        target_installation_id: "inst-cloud",
        authenticated_caller_installation_id: "inst-home",
        verified_recovery_revision: None,
    };
    let env = WarmBootstrapApplyEnv {
        mem: &target_mem,
        general_dir: &target_general,
        state_store: &state_store,
        now_ms: 20_000,
        general_tab_id: Some("tab-1".into()),
        providers: vec![],
    };

    // 1. Wrong account -> rejected with zero writes.
    let wrong = apply_warm_bootstrap(
        base_projection("acct-foreign", "gen-1"),
        &provenance,
        &env,
        |_| Ok(0),
        || Ok(()),
    );
    assert_eq!(wrong.status, WarmBootstrapStatus::Rejected);
    assert_eq!(
        wrong.reject_reason,
        Some(WarmBootstrapRejectReason::WrongAccount)
    );
    assert!(!wrong.readable_owner_context);
    assert!(wrong.warm_console_ready_at_ms.is_none());
    assert!(!target_general.join("identity.md").exists());

    // 2. Valid fresh apply commits once.
    let first = apply_warm_bootstrap(
        base_projection("acct-real", "gen-1"),
        &provenance,
        &env,
        |_| Ok(1),
        || Ok(()),
    );
    assert_eq!(first.status, WarmBootstrapStatus::Applied);
    assert!(first.readable_owner_context);

    // Owner makes a local edit on the cloud machine after bootstrap.
    std::fs::write(
        target_general.join("identity.md"),
        "Owner: Local Cloud Edit",
    )
    .unwrap();

    // 3. Retry with same generation -> AlreadyApplied, preserving local edit (zero overwrite).
    let retry = apply_warm_bootstrap(
        base_projection("acct-real", "gen-1"),
        &provenance,
        &env,
        |_| panic!("retry must not re-apply preferences"),
        || panic!("retry must not re-run console seeding"),
    );
    assert_eq!(retry.status, WarmBootstrapStatus::AlreadyApplied);
    assert_eq!(
        std::fs::read_to_string(target_general.join("identity.md")).unwrap(),
        "Owner: Local Cloud Edit"
    );

    // 4. Stale/different generation after commit -> Rejected(StaleGeneration), zero writes.
    let stale = apply_warm_bootstrap(
        base_projection("acct-real", "gen-2-stale"),
        &provenance,
        &env,
        |_| panic!("stale generation must not apply"),
        || Ok(()),
    );
    assert_eq!(stale.status, WarmBootstrapStatus::Rejected);
    assert_eq!(
        stale.reject_reason,
        Some(WarmBootstrapRejectReason::StaleGeneration)
    );

    // 5. Verified recovery revision on a restored target -> SkippedRestoredState, zero writes.
    let restored_dir = tempfile::tempdir().unwrap();
    let restored_mesh = restored_dir.path().join("mesh");
    std::fs::create_dir_all(&restored_mesh).unwrap();
    std::fs::write(
        restored_mesh.join("cloud_recovery_state.json"),
        r#"{"verified":true,"recovery_revision":"rev-2026-restore-7"}"#,
    )
    .unwrap();
    let rev = read_verified_recovery_revision(restored_dir.path());
    assert_eq!(rev.as_deref(), Some("rev-2026-restore-7"));

    let restored_mem = MemManager::open_in_memory().unwrap();
    restored_mem.seed_bundled_if_needed().unwrap();
    let restored_store = WarmBootstrapStateStore::in_data_dir(restored_dir.path());
    let restored_general = restored_dir.path().join("general");
    let restored_prov = TargetProvenance {
        enrolled_account_id: "acct-real",
        target_installation_id: "inst-cloud",
        authenticated_caller_installation_id: "inst-home",
        verified_recovery_revision: rev.as_deref(),
    };
    let restored_env = WarmBootstrapApplyEnv {
        mem: &restored_mem,
        general_dir: &restored_general,
        state_store: &restored_store,
        now_ms: 30_000,
        general_tab_id: None,
        providers: vec![],
    };
    let skipped = apply_warm_bootstrap(
        base_projection("acct-real", "gen-fresh"),
        &restored_prov,
        &restored_env,
        |_| panic!("recovery revision must skip fresh seeding"),
        || Ok(()),
    );
    assert_eq!(skipped.status, WarmBootstrapStatus::SkippedRestoredState);
    assert_eq!(
        skipped.recovery_revision.as_deref(),
        Some("rev-2026-restore-7")
    );
    assert!(!restored_general.join("identity.md").exists());
}

#[test]
fn partial_outcome_sanitizes_secret_exclusions_and_preserves_valid_seed() {
    let dir = tempfile::tempdir().unwrap();
    let mem = MemManager::open_in_memory().unwrap();
    mem.seed_bundled_if_needed().unwrap();
    let store = WarmBootstrapStateStore::in_data_dir(dir.path());
    let general = dir.path().join("general");

    let secret_token = "ghp_1234567890abcdefghijklmnopqrstuvwx";
    let projection = WarmBootstrapProjection {
        schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
        account_id: "acct-1".into(),
        owner_installation_id: "inst-home".into(),
        target_installation_id: "inst-cloud".into(),
        creation_generation: "gen-partial".into(),
        recovery_revision: None,
        identity: Some(PortableOwnerIdentity {
            identity_md: Some("Owner: Alan Turing".into()),
            soul_md: None,
            nickname: None,
        }),
        wake_up_items: vec![
            PortableMemEntry {
                kind: PortableMemKind::Fact,
                title: "Safe Fact".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "Prefers Rust over bash for parsers".into(),
                importance: 7.0,
                pinned: true,
            },
            PortableMemEntry {
                kind: PortableMemKind::Fact,
                title: "Injected Secret".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: format!("GitHub token {secret_token}"),
                importance: 9.0,
                pinned: true,
            },
        ],
        skills: vec![PortableRegisteredSkill {
            slug: "NUL".into(), // Windows reserved name must be rejected
            title: "Bad Skill".into(),
            version: "1.0.0".into(),
            registry_source: PortableSkillSource::MemSkill,
            summary: None,
            area: None,
            tags: vec![],
            body: "Reserved name".into(),
            importance: 5.0,
            pinned: false,
        }],
        preferences: PortablePreferences::default(),
    };

    let provenance = TargetProvenance {
        enrolled_account_id: "acct-1",
        target_installation_id: "inst-cloud",
        authenticated_caller_installation_id: "inst-home",
        verified_recovery_revision: None,
    };
    let env = WarmBootstrapApplyEnv {
        mem: &mem,
        general_dir: &general,
        state_store: &store,
        now_ms: 40_000,
        general_tab_id: None,
        providers: vec![],
    };

    let result = apply_warm_bootstrap(projection, &provenance, &env, |_| Ok(0), || Ok(()));
    assert_eq!(result.status, WarmBootstrapStatus::Partial);
    assert!(result.readable_owner_context);
    assert_eq!(result.applied_counts.wake_up_items, 1);
    assert_eq!(result.applied_counts.skills, 0);
    assert_eq!(result.excluded.len(), 2);
    let raw_json = serde_json::to_string(&result).unwrap();
    assert!(!raw_json.contains(secret_token));
    assert!(!mem
        .wake_up(&WakeUpOptions::default())
        .unwrap()
        .contains(secret_token));
}

#[test]
fn capability_based_provider_readiness_and_filters() {
    assert!(contains_secret_shaped_content(
        "OPENAI_API_KEY=sk-proj-abcdef1234567890"
    ));
    assert!(contains_secret_shaped_content(
        "-----BEGIN PRIVATE KEY-----\nabc"
    ));
    assert!(contains_forbidden_path(
        "load from ~/.provider-beta/auth.json"
    ));
    assert!(!valid_portable_slug("NUL"));
    assert!(!valid_portable_slug("../escape"));
    assert!(valid_portable_slug("release-checklist_v2"));

    let ready = classify_provider_capability(ProviderCapabilityProbe {
        provider_id: "provider-a".into(),
        profile_id: None,
        display_name: "Provider A".into(),
        installed: true,
        signed_in: Some(true),
        supported_methods: vec!["browser".into()],
        code_input: true,
        login_tab_id: None,
    });
    assert_eq!(ready.state, ProviderSignInState::Ready);
    assert!(!ready.requires_owner_input);

    let needs_signin = classify_provider_capability(ProviderCapabilityProbe {
        provider_id: "provider-b".into(),
        profile_id: Some("antigravity".into()),
        display_name: "ACP (antigravity)".into(),
        installed: true,
        signed_in: Some(false),
        supported_methods: vec!["browser".into(), "apiKey".into()],
        code_input: true,
        login_tab_id: Some("tab-login-1".into()),
    });
    assert_eq!(needs_signin.state, ProviderSignInState::SignInRequired);
    assert!(needs_signin.requires_owner_input);
    assert!(needs_signin.code_input);

    let unsupported = classify_provider_capability(ProviderCapabilityProbe {
        provider_id: "provider-c".into(),
        profile_id: None,
        display_name: "Provider C".into(),
        installed: true,
        signed_in: Some(false),
        supported_methods: vec![],
        code_input: false,
        login_tab_id: None,
    });
    assert_eq!(unsupported.state, ProviderSignInState::Unsupported);
    assert_eq!(
        unsupported.unsupported_reason.as_deref(),
        Some("login_flow_unsupported")
    );
}

struct WarmPhaseProvisioning {
    clock: Arc<StepClock>,
    backend_phases: Mutex<BackendCreatePhases>,
    recorded_events: Mutex<Vec<(String, WarmBootstrapPhaseEvent)>>,
}

impl WarmPhaseProvisioning {
    fn new(clock: Arc<StepClock>) -> Self {
        Self {
            clock,
            backend_phases: Mutex::new(BackendCreatePhases {
                requested: Some(1_700_000_000),
                allocated: Some(1_700_000_002),
                ipv4_bound: Some(1_700_000_003),
                running: Some(1_700_000_005),
                enrolled: Some(1_700_000_008),
                peer_ready: Some(1_700_000_010),
                seed_started: None,
                warm_console_ready: None,
                warm_bootstrap_delta_secs: None,
            }),
            recorded_events: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CloudProvisioningPort for WarmPhaseProvisioning {
    async fn create(
        &self,
        request: &ProvisioningCreateRequest,
    ) -> Result<ProvisionedMachine, CloudError> {
        self.clock.advance(5_000);
        Ok(ProvisionedMachine {
            machine_id: "cm-warm-1".into(),
            provider: "fly_machines".into(),
            endpoint: CloudEndpoint::Direct(SocketAddrV4::new(
                Ipv4Addr::new(198, 51, 100, 77),
                CLOUD_INGRESS_PORT,
            )),
            size: request.size.clone(),
            rate: CloudRate {
                amount: 0.04,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-warm-1".into(),
            lease_expires_at_ms: self.clock.now_ms() + 600_000,
        })
    }

    async fn wake(&self, machine_id: &str, _: &str) -> Result<ProvisionedMachine, CloudError> {
        Ok(ProvisionedMachine {
            machine_id: machine_id.into(),
            provider: "fly_machines".into(),
            endpoint: CloudEndpoint::Direct(SocketAddrV4::new(
                Ipv4Addr::new(198, 51, 100, 77),
                CLOUD_INGRESS_PORT,
            )),
            size: "small".into(),
            rate: CloudRate {
                amount: 0.04,
                currency: "USD".into(),
                credits_per_hour: Some(1.0),
            },
            sleep_mode: CloudSleepMode::Stop,
            lease_id: "lease-warm-2".into(),
            lease_expires_at_ms: self.clock.now_ms() + 600_000,
        })
    }

    async fn sleep(&self, _: &str, _: &str) -> Result<CloudSleepMode, CloudError> {
        Ok(CloudSleepMode::Stop)
    }

    async fn delete(&self, _: &str, _: &str) -> Result<(), CloudError> {
        Ok(())
    }

    async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        _: &str,
    ) -> Result<ProvisionedLease, CloudError> {
        Ok(ProvisionedLease {
            machine_id: machine_id.into(),
            lease_id: lease_id.into(),
            lease_expires_at_ms: self.clock.now_ms() + 600_000,
        })
    }

    async fn usage(&self) -> Result<CloudUsageSummary, CloudError> {
        Ok(CloudUsageSummary::default())
    }

    async fn record_warm_bootstrap_phases(
        &self,
        machine_id: &str,
        phase: WarmBootstrapPhaseEvent,
        _request_id: &str,
    ) -> Result<Option<BackendCreatePhases>, CloudError> {
        self.recorded_events
            .lock()
            .push((machine_id.to_owned(), phase));
        let mut phases = self.backend_phases.lock();
        let now_sec = self.clock.now_ms() / 1_000;
        match phase {
            WarmBootstrapPhaseEvent::SeedStarted => {
                phases.seed_started.get_or_insert(now_sec);
            }
            WarmBootstrapPhaseEvent::WarmConsoleReady => {
                let start = phases
                    .seed_started
                    .ok_or_else(|| CloudError::Conflict("seed_started required".into()))?;
                let ready = phases.warm_console_ready.get_or_insert(now_sec.max(start));
                phases.warm_bootstrap_delta_secs = Some((*ready - start) as f64);
            }
        }
        Ok(Some(phases.clone()))
    }
}

struct WarmEnrollment(Arc<StepClock>);

#[async_trait]
impl CloudEnrollmentPort for WarmEnrollment {
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError> {
        Ok(EnrollmentBinding {
            controller_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA0".into(),
            provider_creation_nonce: Zeroizing::new(
                "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB0".into(),
            ),
            opaque: Arc::new(()),
        })
    }

    async fn enroll(
        &self,
        _: &CloudRouteSpec,
        _: &EnrollmentBinding,
        _: &str,
        _: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        self.0.advance(3_000);
        Ok(EnrollmentOutcome::Enrolled {
            installation_id: "inst-cloud-warm".into(),
            peer_instance_id: Some("peer-cloud-warm".into()),
        })
    }
}

struct WarmPeerControl {
    clock: Arc<StepClock>,
    warm_calls: AtomicUsize,
    readable_context: bool,
}

#[async_trait]
impl CloudPeerControlPort for WarmPeerControl {
    async fn read_idle(&self, _: &CloudMachineRecord) -> Result<PeerBusySnapshot, CloudError> {
        Ok(PeerBusySnapshot::default())
    }

    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        Ok(DrainToken {
            token: format!("drain-{}", record.machine_id),
            entered_at_ms: self.clock.now_ms(),
        })
    }

    async fn reopen_drain(&self, _: &CloudMachineRecord) -> Result<(), CloudError> {
        Ok(())
    }

    async fn update_endpoint(
        &self,
        _: &CloudMachineRecord,
        _: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        _: &CloudMachineRecord,
        _: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        self.clock.advance(2_000);
        Ok(())
    }

    async fn await_peer_ready(&self, _: &CloudMachineRecord, _: u64) -> Result<(), CloudError> {
        Ok(())
    }

    async fn flush_send(&self, _: &CloudMachineRecord, _: &HeldCloudSend) -> FlushWriteOutcome {
        FlushWriteOutcome::Delivered
    }

    async fn apply_warm_bootstrap(
        &self,
        _: &CloudMachineRecord,
        generation: &str,
    ) -> Result<Option<WarmBootstrapApplyResult>, CloudError> {
        self.warm_calls.fetch_add(1, Ordering::SeqCst);
        let started = self.clock.now_ms();
        self.clock.advance(3_000);
        let ended = self.clock.now_ms();
        Ok(Some(WarmBootstrapApplyResult {
            schema_version: WARM_BOOTSTRAP_SCHEMA_VERSION,
            status: if self.readable_context {
                WarmBootstrapStatus::Applied
            } else {
                WarmBootstrapStatus::Failed
            },
            reject_reason: (!self.readable_context)
                .then_some(WarmBootstrapRejectReason::ReadbackUnverified),
            recovery_revision: None,
            creation_generation: generation.into(),
            applied_counts: Default::default(),
            excluded: Vec::new(),
            warnings: Vec::new(),
            readable_owner_context: self.readable_context,
            wake_up_verified: self.readable_context,
            general_tab_id: Some("tab-general-1".into()),
            providers: vec![ProviderSignInCardStatus {
                provider_id: "provider-alpha".into(),
                profile_id: None,
                display_name: "Provider Alpha".into(),
                state: ProviderSignInState::SignInRequired,
                supported_methods: vec!["browser".into()],
                code_input: true,
                login_tab_id: Some("tab-login-alpha".into()),
                requires_owner_input: true,
                unsupported_reason: None,
            }],
            seed_started_at_ms: Some(started),
            warm_console_ready_at_ms: self.readable_context.then_some(ended),
            warm_bootstrap_delta_secs: self.readable_context.then_some(3.0),
            within_budget: self.readable_context.then_some(true),
        }))
    }
}

#[tokio::test]
async fn controller_create_preserves_peer_ready_records_warm_delta_and_wake_never_reseeds() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(1_700_000_000_000);
    let controller = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "ctrl-home".into(),
        clock.clone(),
    );
    let prov = WarmPhaseProvisioning::new(clock.clone());
    let enroll = WarmEnrollment(clock.clone());
    let peer_ctrl = WarmPeerControl {
        clock: clock.clone(),
        warm_calls: AtomicUsize::new(0),
        readable_context: true,
    };

    let receipt = controller
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("warm-box".into()),
                ..Default::default()
            },
            &prov,
            &enroll,
            &peer_ctrl,
        )
        .await
        .unwrap();

    assert_eq!(peer_ctrl.warm_calls.load(Ordering::SeqCst), 1);
    let phases: CloudCreatePhases = receipt.create_phases.clone().expect("create phases");
    assert!(phases.authoritative_backend_phases);
    assert_eq!(phases.requested_at_ms, Some(1_700_000_000_000));
    assert_eq!(phases.provisioned_at_ms, Some(1_700_000_005_000));
    assert_eq!(phases.enrolled_at_ms, Some(1_700_000_008_000));
    assert_eq!(phases.peer_ready_at_ms, Some(1_700_000_010_000));
    assert_eq!(phases.seed_started_at_ms, Some(1_700_000_010_000));
    assert_eq!(phases.warm_console_ready_at_ms, Some(1_700_000_013_000));
    assert_eq!(phases.warm_bootstrap_delta_secs, Some(3.0));
    assert_eq!(phases.within_budget, Some(true));
    assert!(phases.provider_signin_started_at_ms.is_none());

    let events = prov.recorded_events.lock();
    assert_eq!(
        *events,
        vec![
            ("cm-warm-1".to_owned(), WarmBootstrapPhaseEvent::SeedStarted),
            (
                "cm-warm-1".to_owned(),
                WarmBootstrapPhaseEvent::WarmConsoleReady
            ),
        ]
    );
    drop(events);

    // Record human provider sign-in separately; backend warm delta stays unchanged.
    let updated_phases = controller
        .record_provider_signin_timing(
            &receipt.machine_id,
            Some(1_700_000_020_000),
            Some(1_700_000_045_000),
        )
        .unwrap();
    assert_eq!(updated_phases.warm_bootstrap_delta_secs, Some(3.0));
    assert_eq!(
        updated_phases.provider_signin_started_at_ms,
        Some(1_700_000_020_000)
    );
    assert_eq!(
        updated_phases.provider_signin_completed_at_ms,
        Some(1_700_000_045_000)
    );

    // Sleep + wake must never re-run fresh seeding.
    controller
        .sleep(&receipt.machine_id, &prov, &peer_ctrl, false)
        .await
        .unwrap();
    controller
        .wake(&receipt.machine_id, &prov, &peer_ctrl, 30_000)
        .await
        .unwrap();
    assert_eq!(peer_ctrl.warm_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn controller_never_marks_warm_console_ready_when_owner_context_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let clock = StepClock::new(1_700_000_100_000);
    let controller = CloudMachineController::new(
        CloudMachineStore::new(dir.path().join("cloud_machines.json")),
        "ctrl-home".into(),
        clock.clone(),
    );
    let prov = WarmPhaseProvisioning::new(clock.clone());
    let enroll = WarmEnrollment(clock.clone());
    let peer_ctrl = WarmPeerControl {
        clock: clock.clone(),
        warm_calls: AtomicUsize::new(0),
        readable_context: false,
    };

    let receipt = controller
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("unverified-box".into()),
                ..Default::default()
            },
            &prov,
            &enroll,
            &peer_ctrl,
        )
        .await
        .unwrap();

    let phases = receipt.create_phases.expect("phases");
    assert!(phases.peer_ready_at_ms.is_some());
    assert!(phases.seed_started_at_ms.is_some());
    assert!(phases.warm_console_ready_at_ms.is_none());
    assert!(phases.warm_bootstrap_delta_secs.is_none());
    // Backend must only have received seed_started, never warm_console_ready.
    assert_eq!(
        *prov.recorded_events.lock(),
        vec![("cm-warm-1".to_owned(), WarmBootstrapPhaseEvent::SeedStarted)]
    );
    let summary = receipt.warm_bootstrap.expect("summary");
    assert!(!summary.readable_owner_context);
    assert_eq!(summary.status, WarmBootstrapStatus::Failed);
}
