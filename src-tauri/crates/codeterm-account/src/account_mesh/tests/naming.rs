use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::{AccountMeshRefusal, EnrollTrigger};

#[tokio::test]
async fn account_mesh_default_name_does_not_overwrite_concurrent_user_rename() {
    let mut harness = Harness::new().await;
    harness.default_name = Some("BLACKSTAR".into());
    harness.restart().await;
    harness.registry.script.lock().unwrap().rename_race = Some("My desk".into());
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let registry = harness.mesh.list().await.unwrap();
    assert_eq!(registry.rows[0].display_name.as_deref(), Some("My desk"));
    assert_eq!(harness.calls("update"), 0);
}

#[tokio::test]
async fn account_mesh_default_name_on_enrollment() {
    let mut harness = Harness::new().await;
    harness.default_name = Some("BLACKSTAR".into());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let registry = harness.mesh.list().await.unwrap();
    assert_eq!(registry.rows[0].display_name.as_deref(), Some("BLACKSTAR"));
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

#[tokio::test]
async fn account_mesh_default_name_on_sign_in_preserves_user_name() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.logout().await;
    harness.default_name = Some("BLACKSTAR".into());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let registry = harness.mesh.list().await.unwrap();
    assert_eq!(registry.rows[0].display_name.as_deref(), Some("BLACKSTAR"));
    harness
        .mesh
        .rename_self(registry.rows[0].metadata_revision, "My desk".into())
        .await
        .unwrap();
    harness.logout().await;
    harness.default_name = Some("CHANGED-HOST".into());
    harness.restart().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let registry = harness.mesh.list().await.unwrap();
    assert_eq!(registry.rows[0].display_name.as_deref(), Some("My desk"));
}

#[tokio::test]
async fn account_mesh_rename_uses_current_metadata_even_when_the_editor_opened_before_publication()
{
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let opened_revision = harness
        .state()
        .self_installation
        .as_ref()
        .unwrap()
        .metadata_revision;
    *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
    harness.mesh.refresh(EnrollTrigger::Network).await.unwrap();
    harness.log.lock().unwrap().clear();
    let updated = harness
        .mesh
        .rename_self(opened_revision, "Desk".into())
        .await
        .unwrap();
    assert_eq!(updated.previous_metadata_revision, opened_revision + 1);
    assert_eq!(harness.calls("update"), 1);
    assert_eq!(harness.calls("list"), 0);
}

#[tokio::test]
async fn account_mesh_rename_refreshes_a_stale_revision_and_retries_once() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.script.lock().unwrap().update_race = Some("Other name".into());
    let updated = harness.mesh.rename_self(0, "Desk".into()).await.unwrap();
    assert_eq!(updated.display_name, "Desk");
    assert_eq!(updated.previous_metadata_revision, 1);
    assert_eq!(harness.calls("update"), 2);
    assert_eq!(harness.calls("list"), 1);
}

#[tokio::test]
async fn account_mesh_rename_stops_after_a_second_stale_refusal() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness
        .registry
        .script
        .lock()
        .unwrap()
        .reject_update_after_race = true;
    assert_eq!(
        harness.mesh.rename_self(0, "Desk".into()).await,
        Err(AccountMeshRefusal::Forbidden)
    );
    assert_eq!(harness.calls("update"), 2);
    assert_eq!(harness.calls("list"), 1);
}

#[tokio::test]
async fn account_mesh_rename_does_not_retry_a_non_stale_refusal() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.script.lock().unwrap().update = Some(RegistryRefusal::Forbidden);
    assert_eq!(
        harness.mesh.rename_self(0, "Desk".into()).await,
        Err(AccountMeshRefusal::Forbidden)
    );
    assert_eq!(harness.calls("update"), 1);
    assert_eq!(harness.calls("list"), 1);
    harness.registry.script.lock().unwrap().update = Some(RegistryRefusal::RateLimited {
        retry_after_seconds: 15,
    });
    assert_eq!(
        harness.mesh.rename_self(0, "Desk".into()).await,
        Err(AccountMeshRefusal::RateLimited {
            retry_after_seconds: 15
        })
    );
    assert_eq!(harness.calls("update"), 2);
    assert_eq!(harness.calls("list"), 1);
}
