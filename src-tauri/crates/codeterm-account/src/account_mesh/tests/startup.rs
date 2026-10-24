use super::support::*;
use crate::account_mesh::wire::RegistryPresence;
use crate::account_mesh::*;

fn admissions(harness: &Harness) -> usize {
    harness
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry.starts_with("registry:")
                && !entry.starts_with("registry:challenge:")
                && !entry.starts_with("registry:enroll_key:")
        })
        .count()
}

#[tokio::test]
async fn account_mesh_startup_uses_two_admissions_and_network_noops_use_none() {
    let mut harness = Harness::new().await;
    harness.default_name = Some("BLACKSTAR".into());
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(admissions(&harness), 2);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.calls("presence"), 0);
    assert_eq!(harness.calls("update"), 0);
    assert_eq!(harness.calls("publish"), 0);
    let state = harness.state();
    let own = &state.registry.as_ref().unwrap().rows[0];
    assert_eq!(own.display_name.as_deref(), Some("BLACKSTAR"));
    assert_eq!(own.presence, RegistryPresence::Present);
    assert!(own.endpoint.is_some());
    harness.log.lock().unwrap().clear();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(admissions(&harness), 0);

    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(admissions(&harness), 2);
    assert_eq!(harness.calls("renew"), 1);
    assert_eq!(harness.calls("sync"), 1);
    assert_eq!(harness.calls("list"), 0);
}

#[tokio::test]
async fn account_mesh_startup_withdraws_a_stale_endpoint_in_two_admissions() {
    let mut harness = Harness::new().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    *harness.endpoint.0.lock().unwrap() = None;
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.log.lock().unwrap().clear();
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(admissions(&harness), 2);
    assert!(harness.registry.endpoint("owner").is_none());
    assert!(!harness.store.record.lock().unwrap().published);
    harness.log.lock().unwrap().clear();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(admissions(&harness), 0);
}

#[tokio::test]
async fn account_mesh_rename_uses_one_admission_and_updates_both_views() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.log.lock().unwrap().clear();
    let result = harness.mesh.rename_self(0, "My desk".into()).await.unwrap();
    assert_eq!(admissions(&harness), 1);
    assert_eq!(harness.calls("update"), 1);
    let state = harness.state();
    assert_eq!(
        state.self_installation.as_ref().unwrap().metadata_revision,
        result.metadata_revision
    );
    assert_eq!(
        state.registry.as_ref().unwrap().rows[0]
            .display_name
            .as_deref(),
        Some("My desk")
    );
    assert_eq!(
        state.registry.as_ref().unwrap().rows[0].metadata_revision,
        result.metadata_revision
    );
}

#[tokio::test]
async fn account_mesh_network_republishes_an_expired_endpoint() {
    let harness = Harness::new().await;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.clock.set(START + 3599);
    harness
        .mesh
        .with(|inner| inner.published_until = START + 3599);
    harness.log.lock().unwrap().clear();
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("publish"), 1);
    assert!(harness.registry.endpoint("owner").unwrap().expires_at > START + 3599);
}

#[tokio::test]
async fn account_mesh_failed_sync_retries_without_losing_the_default_name() {
    let mut harness = Harness::new().await;
    harness.default_name = Some("BLACKSTAR".into());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.registry.script.lock().unwrap().publication = Some(wire::RegistryRefusal::Offline);
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::Offline)
    );
    harness.registry.script.lock().unwrap().publication = None;
    harness.clock.set(harness.state().retry_at.unwrap());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(harness.calls("sync"), 2);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(
        harness
            .state()
            .self_installation
            .as_ref()
            .unwrap()
            .display_name
            .as_deref(),
        Some("BLACKSTAR")
    );
}

#[test]
fn account_mesh_sync_wire_matches_the_atomic_server_operation() {
    use serde_json::json;
    let operation = wire::MeshOperation::Sync {
        presence: RegistryPresence::Present,
        endpoint: None,
        default_display_name: Some("Desk".into()),
    };
    assert_eq!(
        serde_json::to_value(operation).unwrap(),
        json!({
            "operation": "sync", "presence": "present", "endpoint": null, "default_display_name": "Desk"
        })
    );
    let response = json!({
        "kind": "sync", "installation_id": INSTALLATION, "display_name": "Desk", "presence": "present",
        "publication": "none", "endpoint": null, "previous_metadata_revision": 0, "metadata_revision": 1,
        "slot_revision": 1, "registry_revision": 1,
        "registry": {"registry_revision": 1, "rows": [], "cursor": null}
    });
    assert!(matches!(
        serde_json::from_value::<wire::MeshResult>(response.clone()).unwrap(),
        wire::MeshResult::Sync(_)
    ));
    let mut malformed = response;
    malformed["presence"] = json!("offline");
    assert!(serde_json::from_value::<wire::MeshResult>(malformed).is_err());
}

#[tokio::test]
async fn account_mesh_startup_with_a_late_listener_uses_at_most_three_admissions() {
    let mut harness = Harness::new().await;
    harness.default_name = Some("BLACKSTAR".into());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(admissions(&harness), 2);
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(admissions(&harness), 3);
    assert_eq!(harness.calls("publish"), 1);
    assert_eq!(harness.calls("list"), 0);
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    assert_eq!(admissions(&harness), 3);
    assert!(harness.registry.endpoint("owner").is_some());
}

#[tokio::test]
async fn account_mesh_startup_keeps_the_page_cursor_without_automatic_admissions() {
    let harness = Harness::new().await;
    harness.registry.script.lock().unwrap().sync_cursor = Some(wire::MeshCursor {
        after_installation_id: INSTALLATION.into(),
        registry_revision: 9,
    });
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(admissions(&harness), 2);
    assert!(harness.state().registry.as_ref().unwrap().cursor.is_some());
    assert!(harness.mesh.list().await.unwrap().cursor.is_none());
    assert_eq!(harness.calls("list"), 1);
}
