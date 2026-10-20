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
            plan: codeterm_analytics::PlanKey::Pro
        }]
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
async fn product_update_installed_requires_running_target_with_kind_only() {
    let sink = Sink::new(Consent::On).await;
    let path = sink.dir.path().join("update.json");
    crate::service::analytics::update_confirm::stage(&path, "1.1.0-rc.2").unwrap();
    crate::service::analytics::update_confirm::stage(&path, "1.1.0-rc.2").unwrap();
    let worker = sink.worker.clone();
    let receipt = path.clone();
    tokio::task::spawn_blocking(move || {
        crate::service::analytics::update_confirm::confirm(&receipt, "1.0.0-rc.1", &worker)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(sink.events().await.is_empty());
    let worker = sink.worker.clone();
    tokio::task::spawn_blocking(move || {
        crate::service::analytics::update_confirm::confirm(&path, "1.1.0-rc.2", &worker).unwrap();
        crate::service::analytics::update_confirm::confirm(&path, "1.1.0-rc.2", &worker).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(
        sink.events().await,
        vec![Event::UpdateInstalled {
            kind: codeterm_analytics::UpdateKind::App
        }]
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
    let machines = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cache = machines.clone();
    mesh.bind_pairing_observers(
        mesh_observer(sink.worker.clone()),
        Arc::new(move |count| {
            cache.store(count, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    assert_eq!(machines.load(std::sync::atomic::Ordering::Relaxed), 1);
    let peer: PairedPeer = serde_json::from_value(serde_json::json!({"instance_id":"private-peer","instance_name":"private-name","addresses":[],"api_port":7685,"public_key":"private-key","paired_at":"private-date","last_seen":null,"version":null})).unwrap();
    mesh.complete_explicit_pairing(peer);
    assert!(mesh.save_with(|| Err("disk failed".into())).is_err());
    assert!(sink.events().await.is_empty());
    assert_eq!(machines.load(std::sync::atomic::Ordering::Relaxed), 1);
    mesh.save().unwrap();
    mesh.save().unwrap();
    assert_eq!(mesh.paired_peers.len(), 1);
    assert_eq!(machines.load(std::sync::atomic::Ordering::Relaxed), 2);
    mesh.paired_peers.clear();
    mesh.save().unwrap();
    assert_eq!(machines.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(
        sink.events().await,
        vec![Event::MeshPeerPaired { mesh_machines: 2 }]
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
        "/../../../scripts/frontend-release/fixtures/frontend-manifest.json"
    ));
    let signature = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../scripts/frontend-release/fixtures/frontend-manifest.json.minisig"
    ));
    let archive = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../scripts/frontend-release/fixtures/frontend-v1.0.0.tar.gz"
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
    bundles.bind_installation_observer(Arc::new(move || {
        worker
            .record_nowait(Event::UpdateInstalled {
                kind: codeterm_analytics::UpdateKind::Frontend,
            })
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
            kind: codeterm_analytics::UpdateKind::Frontend
        }]
    );
}

#[tokio::test]
async fn product_plan_created_follows_persistence_only() {
    use codeterm_plan::{plan::OrchestratorRef, service_plan::PlanStore};
    let sink = Sink::new(Consent::On).await;
    let store = PlanStore::new_for_test(sink.dir.path());
    let worker = sink.worker.clone();
    store.bind_product_observer(Arc::new(move || {
        worker.record_nowait(Event::PlanCreated {}).unwrap();
    }));
    assert!(store
        .create_plan_for_team(
            "private".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("private"),
            "".into(),
            vec![],
            1
        )
        .is_err());
    assert!(sink.events().await.is_empty());
    let mut plan = store
        .create_plan_for_team(
            "private".into(),
            sink.dir.path().display().to_string(),
            OrchestratorRef::from_tab("private"),
            "private-team".into(),
            vec![],
            1,
        )
        .unwrap();
    store.save(&mut plan, 2).unwrap();
    assert_eq!(sink.events().await, vec![Event::PlanCreated {}]);
}

#[tokio::test]
async fn product_agent_spawned_and_workspace_counts_use_fake_sink() {
    use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
    let sink = Sink::new(Consent::On).await;
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.role = Some(AgentRole::Worker);
    let event =
        agent_spawned(tab.agent_type.as_deref(), tab.role.map(|role| role.label())).unwrap();
    sink.worker.record_nowait(event).unwrap();
    sink.worker
        .record_activity_nowait(|| mux.workspace_count())
        .unwrap();
    let initial_workspaces = count(mux.workspace_count());
    let cwd = sink
        .dir
        .path()
        .join("private-workspace")
        .display()
        .to_string();
    let (_, workspaces) = crate::service::workspace::create_workspace_with_count(
        &mut mux,
        "private".into(),
        Some(cwd),
    );
    sink.worker
        .record_nowait(Event::WorkspaceCreated { workspaces })
        .unwrap();
    assert_eq!(workspaces, initial_workspaces + 1);
    assert_eq!(
        sink.events().await,
        vec![
            Event::AgentSpawned {
                provider_id: ProviderId::Codex,
                role: Role::Worker
            },
            Event::ActiveDay {
                mesh_machines: 1,
                workspaces: initial_workspaces
            },
            Event::WorkspaceCreated { workspaces },
        ]
    );
}

#[tokio::test]
async fn product_feature_boundary_uses_fake_sink_and_consent_gate() {
    for consent in [Consent::On, Consent::Off] {
        let sink = Sink::new(consent).await;
        for feature in [
            codeterm_analytics::Feature::MemorySaved,
            codeterm_analytics::Feature::MemorySaved,
            codeterm_analytics::Feature::ThemeChanged,
        ] {
            sink.worker
                .record_nowait(Event::FeatureUsed { feature })
                .unwrap();
        }
        let expected = if consent == Consent::On {
            vec![
                Event::FeatureUsed {
                    feature: codeterm_analytics::Feature::MemorySaved,
                },
                Event::FeatureUsed {
                    feature: codeterm_analytics::Feature::ThemeChanged,
                },
            ]
        } else {
            vec![]
        };
        assert_eq!(sink.events().await, expected);
    }
}
