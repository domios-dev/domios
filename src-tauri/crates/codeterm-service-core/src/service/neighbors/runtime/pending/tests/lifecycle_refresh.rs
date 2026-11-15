use super::*;

#[tokio::test]
async fn watcher_reloads_permission_context_and_binding_before_publishing() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let first = mux.create_tab(&workspace).unwrap();
    let second = mux.create_tab(&workspace).unwrap();
    for tab in [&first, &second] {
        mux.get_tab_mut(tab).unwrap().agent_type = Some("fixture".into());
    }
    let fixture = ServiceFixture::new(mux);
    let (session, shutdown) = receiving_session(&fixture, Some(first.clone())).await;
    let mut settings = Settings::default();
    settings
        .bindings
        .insert(session.friendship.clone(), first.clone());
    session
        .metadata
        .save_settings(settings.clone())
        .await
        .unwrap();
    let (published, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let listener = fixture
        .ui
        .listen("neighbor-concierge-status", move |value| {
            published.send(value.clone()).unwrap();
        });
    let watch = watch_lifecycle(&session);
    tokio::pin!(watch);
    let initial = tokio::select! {
        _ = &mut watch => panic!("watch ended before its initial projection"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(initial["state"], "concierge_access_needed");
    settings
        .permissions
        .insert(session.friendship.clone(), Permission::default());
    settings.initial_contexts.insert(
        session.friendship.clone(),
        crate::service::neighbors::storage::InitialContext {
            gateway_tab: first.clone(),
            digest: "updated-context".into(),
            launch_options: Some(crate::service::neighbors::storage::ConciergeOptions {
                reasoning_effort: Some("updated-effort".into()),
                service_tier: None,
            }),
        },
    );
    session
        .metadata
        .save_settings(settings.clone())
        .await
        .unwrap();
    session
        .service
        .events
        .publish_generic("agent-runtime-state", json!({"tab_id":first}));
    let reviewed = tokio::select! {
        _ = &mut watch => panic!("watch ended before access review publication"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(reviewed["state"], "concierge_starting");
    assert_eq!(reviewed["initial_context_digest"], "updated-context");
    assert_eq!(reviewed["reasoning_effort"], "updated-effort");
    settings
        .bindings
        .insert(session.friendship.clone(), second.clone());
    settings
        .initial_contexts
        .get_mut(&session.friendship)
        .unwrap()
        .gateway_tab = second.clone();
    session
        .metadata
        .save_settings(settings.clone())
        .await
        .unwrap();
    session
        .service
        .events
        .publish_generic("mux-state-changed", json!({}));
    let rebound = tokio::select! {
        _ = &mut watch => panic!("watch ended before binding publication"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(rebound["gateway_tab"], second);
    settings.permissions.remove(&session.friendship);
    session.metadata.save_settings(settings).await.unwrap();
    session.service.events.publish_generic(
        "neighbor-permission-changed",
        json!({"friendship_id":session.friendship}),
    );
    let revoked = tokio::select! {
        _ = &mut watch => panic!("watch ended before changed permission publication"),
        value = tokio::time::timeout(std::time::Duration::from_secs(5), observed.recv()) => value.unwrap().unwrap(),
    };
    assert_eq!(revoked["state"], "concierge_access_needed");
    assert_eq!(revoked["gateway_tab"], second);
    shutdown.send(true).unwrap();
    watch.await;
    fixture.ui.unlisten(listener);
}
