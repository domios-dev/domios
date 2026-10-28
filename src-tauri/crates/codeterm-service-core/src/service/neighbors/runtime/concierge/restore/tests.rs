use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn older_concierge_is_replaced_in_friend_folder_without_losing_permissions_or_model() {
    for tagged in [false, true] {
        let (mut mux, general) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let old = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&old).unwrap();
        tab.agent_type = Some("advisory-provider".into());
        tab.agent_session_model = Some("opaque-model".into());
        tab.restored_from_snapshot = true;
        tab.cwd = Some("old-general-folder".into());
        let fixture = ServiceFixture::new(mux);
        let service = &fixture.service;
        let friend = "a".repeat(64);
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let permission = Permission {
            allowed: true,
            scope: "opaque scope".into(),
            allow_memory: false,
            ..Default::default()
        };
        let mut settings = crate::service::neighbors::storage::Settings::default();
        settings.bindings.insert(friend.clone(), old.clone());
        settings
            .permissions
            .insert(friend.clone(), permission.clone());
        metadata.save_settings(settings).await.unwrap();
        if tagged {
            service.tag_neighbor_root("owner", &friend, &old).unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = calls.clone();
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |provider, model, cwd, access| async move {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(provider.as_deref(), Some("advisory-provider"));
                    assert_eq!(model.as_deref(), Some("opaque-model"));
                    assert_eq!(access, MachineAccess::ReadOnly);
                    let mut mux = service.mux.write();
                    let id = mux.create_tab(&workspace).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = provider;
                    tab.agent_session_model = model;
                    tab.cwd = Some(cwd);
                    Ok(id)
                },
            )
            .await
            .unwrap();
        let saved = metadata.settings().await.unwrap();
        let new = &saved.bindings[&friend];
        assert_ne!(new, &old);
        assert_eq!(
            serde_json::to_value(&saved.permissions[&friend]).unwrap(),
            serde_json::to_value(&permission).unwrap()
        );
        let projected = service
            .neighbor_permission_projection(&metadata, &friend, &permission, &saved)
            .unwrap();
        assert_eq!(projected["folder_enforcement"]["writes"], "advisory");
        assert_eq!(
            std::path::Path::new(projected["effective_folder"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            metadata
                .friend_folder(&friend)
                .unwrap()
                .canonicalize()
                .unwrap()
        );
        assert!(service.mux.read().get_tab(&old).is_none());
        assert!(service
            .mux
            .read()
            .get_tab(new)
            .unwrap()
            .neighbor_authority
            .is_some());
        service
            .restore_neighbor_concierge_with(&metadata, "owner", &friend, |_, _, _, _| async {
                Err("matching launch must not respawn".into())
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn failed_replacement_keeps_the_old_binding_and_permissions() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let old = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&old).unwrap().agent_type = Some("advisory-provider".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let friend = "b".repeat(64);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend.clone(), old.clone());
    settings
        .permissions
        .insert(friend.clone(), Permission::default());
    metadata.save_settings(settings.clone()).await.unwrap();
    assert!(service
        .restore_neighbor_concierge_with(&metadata, "owner", &friend, |_, _, _, _| async {
            Err("injected launch failure".into())
        })
        .await
        .is_err());
    assert_eq!(
        serde_json::to_value(metadata.settings().await.unwrap()).unwrap(),
        serde_json::to_value(settings).unwrap()
    );
    assert!(service.mux.read().get_tab(&old).is_some());
}
