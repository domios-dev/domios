use super::super::*;
use codeterm_analytics::{
    sinks::fake::{Fake, Recording},
    Consent,
};
use std::sync::{Arc, Mutex};
struct Sink {
    dir: tempfile::TempDir,
    worker: Worker,
    recording: Arc<Mutex<Recording>>,
}
impl Sink {
    async fn new(consent: Consent) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let recording = Arc::new(Mutex::new(Recording::default()));
        let worker = Worker::start_with_sink(
            dir.path().join("settings.json"),
            Config::default(),
            Box::new(Fake {
                recording: recording.clone(),
            }),
        )
        .unwrap();
        worker.set_consent(consent).await.unwrap();
        worker.flush().await.unwrap();
        recording.lock().unwrap().batches.clear();
        Self {
            dir,
            worker,
            recording,
        }
    }
    async fn events(&self) -> Vec<Event> {
        self.worker.flush().await.unwrap();
        self.recording
            .lock()
            .unwrap()
            .batches
            .iter()
            .flatten()
            .map(|item| item.event.clone())
            .collect()
    }
}
#[tokio::test]
async fn product_update_installed_requires_running_target_and_counts_attempts() {
    let sink = Sink::new(Consent::On).await;
    let path = sink.dir.path().join("update.json");
    crate::update::analytics::stage(&path, "1.0.0-rc.1", "1.1.0-rc.2").unwrap();
    crate::update::analytics::stage(&path, "1.0.0-rc.1", "1.1.0-rc.2").unwrap();
    let worker = sink.worker.clone();
    let receipt = path.clone();
    tokio::task::spawn_blocking(move || {
        crate::update::analytics::confirm(&receipt, "1.0.0-rc.1", &worker)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(sink.events().await.is_empty());
    let worker = sink.worker.clone();
    tokio::task::spawn_blocking(move || {
        crate::update::analytics::confirm(&path, "1.1.0-rc.2", &worker).unwrap();
        crate::update::analytics::confirm(&path, "1.1.0-rc.2", &worker).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(
        sink.events().await,
        vec![Event::UpdateInstalled {
            from_version: Version::new(1, 0, 0),
            to_version: Version::new(1, 1, 0),
            kind: codeterm_analytics::UpdateKind::App,
            attempt_count: 2
        }]
    );
}
#[tokio::test]
async fn product_plan_created_and_dispatch_follow_persisted_ack_only() {
    use codeterm_plan::{
        plan::{ExecutionIntent as Intent, OrchestratorRef, TaskLaneMetadata},
        service_plan::{NewTask, PlanStore},
    };
    let sink = Sink::new(Consent::On).await;
    let store = PlanStore::new_for_test(sink.dir.path());
    store.bind_product_observer(plan_observer(sink.worker.clone()));
    assert!(store
        .create_plan_for_team(
            "private-plan".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("private-owner"),
            "".into(),
            vec![],
            1
        )
        .is_err());
    let mut plan = store
        .create_plan_for_team(
            "private-plan".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("private-owner"),
            "private-team".into(),
            vec![],
            1,
        )
        .unwrap();
    store.save(&mut plan, 2).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "private-task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let attempt = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task,
            "private-worker",
            "Worker",
            TaskLaneMetadata::default(),
            Intent::ReadOnly,
            None,
            Some("codex"),
            4,
        )
        .unwrap();
    assert_eq!(sink.events().await, vec![Event::PlanCreated {}]);
    store
        .finish_task_dispatch(
            &plan.id,
            &task,
            "private-worker",
            &attempt,
            "private-session",
            "private-nonce",
            TaskLaneMetadata::default(),
            5,
        )
        .unwrap();
    assert!(store
        .acknowledge_task_attempt(&plan.id, &task, "wrong-worker", &attempt, 6)
        .is_err());
    assert_eq!(sink.events().await, vec![Event::PlanCreated {}]);
    store
        .acknowledge_task_attempt(&plan.id, &task, "private-worker", &attempt, 7)
        .unwrap();
    store
        .acknowledge_task_attempt(&plan.id, &task, "private-worker", &attempt, 8)
        .unwrap();
    assert!(store.get(&plan.id).unwrap().tasks[0].execution_attempts[0]
        .acknowledged_at
        .is_some());
    assert_eq!(
        sink.events().await,
        vec![
            Event::PlanCreated {},
            Event::TaskDispatched {
                execution_intent: codeterm_analytics::ExecutionIntent::ReadOnly
            }
        ]
    );
}
#[tokio::test]
async fn product_mesh_peer_paired_requires_successful_save_and_deduplicates() {
    use codeterm_mesh::{
        types::{MeshIdentity, PairedPeer},
        MeshManager,
    };
    let sink = Sink::new(Consent::On).await;
    let mut mesh = MeshManager::in_memory(
        MeshIdentity {
            instance_id: "private-self".into(),
            private_key: None,
            public_key: None,
        },
        "private-name".into(),
        7686,
    );
    mesh.bind_pairing_observer(mesh_observer(sink.worker.clone()));
    let peer: PairedPeer = serde_json::from_value(serde_json::json!({"instance_id":"private-peer","instance_name":"private-name","addresses":[],"api_port":7685,"public_key":"private-key","paired_at":"private-date","last_seen":null,"version":null})).unwrap();
    mesh.complete_explicit_pairing(peer);
    assert!(mesh.save_with(|| Err("disk failed".into())).is_err());
    assert!(sink.events().await.is_empty());
    mesh.save().unwrap();
    mesh.save().unwrap();
    assert_eq!(mesh.paired_peers.len(), 1);
    assert_eq!(sink.events().await, vec![Event::MeshPeerPaired {}]);
}
#[tokio::test]
async fn product_agent_restored_requires_current_nonce_and_authenticated_target() {
    use crate::service::boot_stage::BootStage;
    let sink = Sink::new(Consent::On).await;
    let mut restores = restore::Restores::default();
    restores.begin(
        "private-tab",
        1,
        Some("private-nonce".into()),
        Some("private-target".into()),
        Some("codex"),
    );
    assert!(restores
        .settle(
            "private-tab",
            Some("stale"),
            Some("private-target"),
            true,
            BootStage::AgentReady
        )
        .is_none());
    assert!(restores
        .settle(
            "private-tab",
            Some("private-nonce"),
            Some("private-target"),
            false,
            BootStage::AgentReady
        )
        .is_none());
    assert!(restores
        .settle(
            "private-tab",
            Some("private-nonce"),
            Some("wrong"),
            true,
            BootStage::AgentReady
        )
        .is_none());
    sink.worker
        .record_nowait(
            restores
                .settle(
                    "private-tab",
                    Some("private-nonce"),
                    Some("private-target"),
                    true,
                    BootStage::AgentReady,
                )
                .unwrap(),
        )
        .unwrap();
    assert!(restores
        .settle(
            "private-tab",
            Some("private-nonce"),
            Some("private-target"),
            true,
            BootStage::AgentReady
        )
        .is_none());
    restores.begin("private-tab", 2, Some("fresh".into()), None, Some("codex"));
    sink.worker
        .record_nowait(
            restores
                .settle(
                    "private-tab",
                    Some("fresh"),
                    Some("new"),
                    true,
                    BootStage::AgentReady,
                )
                .unwrap(),
        )
        .unwrap();
    restores.begin(
        "private-tab",
        3,
        Some("failed".into()),
        Some("target".into()),
        Some("codex"),
    );
    sink.worker
        .record_nowait(
            restores
                .settle(
                    "private-tab",
                    Some("failed"),
                    None,
                    false,
                    BootStage::SpawnFailed,
                )
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        sink.events().await,
        vec![
            Event::AgentRestored {
                provider_id: ProviderId::Codex,
                outcome: codeterm_analytics::RestoreOutcome::Resumed
            },
            Event::AgentRestored {
                provider_id: ProviderId::Codex,
                outcome: codeterm_analytics::RestoreOutcome::FreshLaunch
            },
            Event::AgentRestored {
                provider_id: ProviderId::Codex,
                outcome: codeterm_analytics::RestoreOutcome::Failed
            }
        ]
    );
}
#[tokio::test]
async fn product_subscription_changed_uses_authoritative_refresh_and_closed_enums() {
    use codeterm_billing::{Billing, Plan, RefreshEvent, Status};
    let sink = Sink::new(Consent::On).await;
    let provider = Arc::new(codeterm_billing::adapters::fake::Fake::default());
    let billing =
        Billing::new(provider.clone()).with_observer(billing_observer(sink.worker.clone()));
    let session = codeterm_account::Session {
        identity: codeterm_account::Identity {
            uid: "private-owner".into(),
            email: None,
            display_name: None,
            provider: "fake".into(),
        },
        id_token: codeterm_account::Secret::new("private-token"),
    };
    let now = std::time::Instant::now();
    billing
        .refresh(Some(&session), RefreshEvent::SignIn, now)
        .await;
    billing
        .refresh(Some(&session), RefreshEvent::CheckoutReturned, now)
        .await;
    *provider.value.lock() = Err("offline".into());
    billing
        .refresh(Some(&session), RefreshEvent::CheckoutReturned, now)
        .await;
    *provider.value.lock() = Ok(codeterm_billing::Entitlements {
        plan: Plan::Pro,
        status: Status::PastDue,
        ..Default::default()
    });
    let result = billing
        .refresh(Some(&session), RefreshEvent::CheckoutReturned, now)
        .await;
    assert_eq!(result.entitlements.plan, Plan::Pro);
    assert_eq!(
        sink.events().await,
        vec![
            subscription_event(Plan::Free, Status::Active),
            subscription_event(Plan::Pro, Status::PastDue)
        ]
    );
}
#[tokio::test]
async fn product_frontend_update_installed_requires_persisted_health_of_all_clients() {
    use codeterm_frontend_bundle::{
        activation::Activation, manifest, store::Store, FrontendBundles,
    };
    let sink = Sink::new(Consent::On).await;
    let root = sink.dir.path().join("frontend");
    let metadata = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/frontend-release/fixtures/frontend-manifest.json"
    ));
    let signature = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/frontend-release/fixtures/frontend-manifest.json.minisig"
    ));
    let archive = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/frontend-release/fixtures/frontend-v1.0.0.tar.gz"
    ));
    let key = "RWRmcm9udGVuZNEdQEUZEh9CtfmYUwGDKXT9+qbRXfdx90/9a+hv/8OA";
    let verified = manifest::verify(metadata, signature, archive, key, 2, false).unwrap();
    let mut store = Store::open(&root).unwrap();
    store.install(&verified, signature, archive).unwrap();
    store
        .save(&Activation {
            pending: Some(verified.manifest().clone()),
            ..Default::default()
        })
        .unwrap();
    drop(store);
    let bundles = FrontendBundles::open(
        &root,
        2,
        "0.9.0",
        Some(key.into()),
        "https://example.invalid/".into(),
        false,
        Arc::new(|_, _| {}),
    )
    .unwrap();
    let worker = sink.worker.clone();
    bundles.bind_installation_observer(Arc::new(move |installation| {
        worker
            .record_nowait(frontend_event(installation).unwrap())
            .unwrap();
    }));
    let release = &verified.manifest().release_id;
    let activated = bundles
        .activate(
            release,
            vec!["client-a".into(), "client-b".into()],
            u64::MAX,
        )
        .unwrap();
    let generation = activated["generation"].as_u64().unwrap();
    let token_a = activated["claims"]["client-a"].as_str().unwrap();
    let token_b = activated["claims"]["client-b"].as_str().unwrap();
    assert!(bundles
        .healthy(release, generation, "client-a", "wrong")
        .is_err());
    bundles
        .healthy(release, generation, "client-a", token_a)
        .unwrap();
    assert!(sink.events().await.is_empty());
    bundles
        .healthy(release, generation, "client-b", token_b)
        .unwrap();
    bundles
        .healthy(release, generation, "client-b", token_b)
        .unwrap();
    assert_eq!(
        Store::open(&root)
            .unwrap()
            .load()
            .unwrap()
            .active
            .unwrap()
            .release_id,
        *release
    );
    assert_eq!(
        sink.events().await,
        vec![Event::UpdateInstalled {
            from_version: Version::new(0, 9, 0),
            to_version: Version::new(1, 0, 0),
            kind: codeterm_analytics::UpdateKind::Frontend,
            attempt_count: 1
        }]
    );
}
#[tokio::test]
async fn product_checkout_started_records_browser_open_with_closed_plan_only() {
    let sink = Sink::new(Consent::On).await;
    let billing =
        codeterm_billing::Billing::new(Arc::new(codeterm_billing::adapters::fake::Fake::default()))
            .with_observer(billing_observer(sink.worker.clone()));
    let session = codeterm_account::Session {
        identity: codeterm_account::Identity {
            uid: "private-owner".into(),
            email: None,
            display_name: None,
            provider: "fake".into(),
        },
        id_token: codeterm_account::Secret::new("private-token"),
    };
    billing.checkout_url(&session, "pro").await.unwrap();
    assert!(billing
        .checkout_url(&session, "private-plan")
        .await
        .is_err());
    billing.browser_opened(&session);
    assert!(sink.events().await.is_empty());
    billing.checkout_browser_opened(&session, codeterm_billing::Plan::Pro);
    assert_eq!(
        sink.events().await,
        vec![Event::CheckoutStarted {
            plan_key: codeterm_analytics::PlanKey::Pro
        }]
    );
}
#[tokio::test]
async fn product_domain_observers_do_not_send_with_consent_off() {
    let sink = Sink::new(Consent::Off).await;
    plan_observer(sink.worker.clone())(
        codeterm_plan::service_plan::product_events::ProductEvent::PlanCreated,
    );
    mesh_observer(sink.worker.clone())();
    billing_observer(sink.worker.clone())(codeterm_billing::ProductEvent::CheckoutStarted {
        plan: codeterm_billing::Plan::Pro,
    });
    billing_observer(sink.worker.clone())(codeterm_billing::ProductEvent::SubscriptionChanged {
        plan: codeterm_billing::Plan::Pro,
        status: codeterm_billing::Status::Active,
    });
    sink.worker
        .record_nowait(
            frontend_event(codeterm_frontend_bundle::Installation {
                from_version: "1.0.0".into(),
                to_version: "1.1.0".into(),
                attempt_count: 1,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(sink.events().await.is_empty());
}
#[tokio::test]
async fn product_remote_dispatch_reports_declared_unspecified_policy_once() {
    use codeterm_plan::{
        plan::{OrchestratorRef, RemoteDispatchState, TaskExecutionAttempt},
        service_plan::{NewTask, PlanStore},
    };
    let sink = Sink::new(Consent::On).await;
    let store = PlanStore::new_for_test(sink.dir.path());
    store.bind_product_observer(plan_observer(sink.worker.clone()));
    let plan = store
        .create_plan_for_team(
            "private".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "private".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            plan.tasks[0].execution_attempts.push(TaskExecutionAttempt {
                id: "attempt".into(),
                executor_tab_id: "worker".into(),
                mesh_peer: Some("peer".into()),
                remote_dispatch_state: Some(RemoteDispatchState::AwaitingWorkerAck),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    assert!(store
        .acknowledge_remote_dispatch(&plan.id, &task, "attempt", "wrong", "worker", 4)
        .is_err());
    assert!(store
        .acknowledge_remote_dispatch(&plan.id, &task, "attempt", "peer", "worker", 5)
        .unwrap());
    assert!(!store
        .acknowledge_remote_dispatch(&plan.id, &task, "attempt", "peer", "worker", 6)
        .unwrap());
    assert_eq!(
        sink.events().await,
        vec![
            Event::PlanCreated {},
            Event::TaskDispatched {
                execution_intent: codeterm_analytics::ExecutionIntent::Unspecified
            }
        ]
    );
}
#[test]
fn product_billing_enum_contract_matches_serialized_authority() {
    use codeterm_billing::{Plan, Status};
    for plan in [Plan::Free, Plan::Pro] {
        assert_eq!(
            serde_json::to_value(plan).unwrap(),
            serde_json::to_value(billing_plan(plan)).unwrap()
        );
        for status in [
            Status::Active,
            Status::Trialing,
            Status::PastDue,
            Status::Canceled,
            Status::Revoked,
            Status::Incomplete,
            Status::Unpaid,
            Status::Paused,
        ] {
            let payload = serde_json::to_value(subscription_event(plan, status)).unwrap();
            assert_eq!(
                payload["event_properties"]["status"],
                serde_json::to_value(status).unwrap()
            );
        }
    }
}
#[tokio::test]
async fn product_agent_restore_transition_emits_from_authenticated_tab_state() {
    use crate::service::boot_stage::BootStage;
    let sink = Sink::new(Consent::On).await;
    let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("exact-session".into());
    tab.session_binding_nonce = Some("exact-nonce".into());
    tab.reported_session_binding_nonce = Some("old-nonce".into());
    restore::begin(
        &id,
        42,
        Some("exact-nonce".into()),
        Some("exact-session".into()),
        Some("codex"),
    );
    assert!(restore::transition(&id, tab, BootStage::AgentReady).is_none());
    restore::bind(&id, 41, "stale-generation");
    tab.reported_session_binding_nonce = Some("exact-nonce".into());
    sink.worker
        .record_nowait(restore::transition(&id, tab, BootStage::AgentReady).unwrap())
        .unwrap();
    assert!(restore::transition(&id, tab, BootStage::AgentReady).is_none());
    assert_eq!(
        sink.events().await,
        vec![Event::AgentRestored {
            provider_id: ProviderId::Codex,
            outcome: codeterm_analytics::RestoreOutcome::Resumed
        }]
    );
}
