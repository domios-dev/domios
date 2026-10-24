use super::support::*;
use crate::account_mesh::EnrollTrigger;

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
    assert_eq!(harness.calls("update"), 1);
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
