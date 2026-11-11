use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn sidebar_projection_keeps_known_receiver_rows_after_concierge_unbinding() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let friendship = "a".repeat(64);
    fixture
        .seed_neighbor_concierge(
            &tab,
            &friendship,
            Permission {
                allowed: true,
                scope: "private grant".into(),
                ..Default::default()
            },
        )
        .await;
    let list = json!({"access":"paid", "friends":[
        {"friendship_id": friendship, "displayName":"Opaque Friend", "photoURL":"https://example.test/avatar.png", "private_key":"secret"},
        {"friendship_id": "b".repeat(64), "displayName":"Unbound Friend"}
    ]});
    fixture.service.remember_neighbor_names(&list);
    fixture.service.remember_neighbor_registry(&list);
    assert_eq!(
        fixture.service.neighbor_sidebar_projection().await.unwrap(),
        json!({(friendship.clone()): {"tab_id":tab, "friend_display_name":"Opaque Friend", "friend_photo_url":"https://example.test/avatar.png", "paused":false, "access":"paid",
            "receiver":{"receiving_here":true,"receiver_machine_label":"Fixture","receiving_enabled":true,"revision":1,"is_me":true}}})
    );
    let session = fixture
        .service
        .account()
        .await
        .unwrap()
        .session()
        .await
        .unwrap();
    fixture
        .service
        .observe_neighbor_pause(&session.identity.uid, &friendship, true);
    assert_eq!(
        fixture.service.neighbor_sidebar_projection().await.unwrap()[&friendship]["paused"],
        true
    );
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&friendship, Some(false), 2);
    let moved = fixture.service.neighbor_sidebar_projection().await.unwrap();
    assert_eq!(
        moved[&friendship]["receiver"],
        json!({
            "receiving_here":false,"receiver_machine_label":"Fixture","receiving_enabled":true,"revision":2,"is_me":false
        })
    );
    let metadata = Metadata::new(
        fixture.service.account_data_root().unwrap(),
        &session.identity.uid,
    );
    let mut settings = metadata.settings().await.unwrap();
    settings.bindings.remove(&friendship);
    metadata.save_settings(settings).await.unwrap();
    let unbound = fixture.service.neighbor_sidebar_projection().await.unwrap();
    assert!(unbound[&friendship]["tab_id"].is_null());
    assert_eq!(
        unbound[&friendship]["receiver"],
        moved[&friendship]["receiver"]
    );
    assert_eq!(unbound[&friendship]["friend_display_name"], "Opaque Friend");
    fixture.service.forget_neighbor_registry();
    let projection = fixture.service.neighbor_sidebar_projection().await.unwrap();
    assert!(projection[&friendship]["friend_photo_url"].is_null());
    assert!(projection[&friendship]["access"].is_null());
}

#[test]
fn registry_cache_retains_explicit_receiver_authority_including_malformed_projection() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let friendship = "a".repeat(64);
    let projection = json!({"installation_id":"elsewhere","revision":9});
    fixture.service.remember_neighbor_registry(&json!({"access":"paid","friends":[{"friendship_id":friendship,"my_receiver":projection,"peer_receiver":{"enc_key_id":"opaque-key","revision":2,"receiving":true}}]}));
    let cached = fixture
        .service
        .neighbor_cell()
        .registry_snapshot
        .lock()
        .clone()
        .unwrap();
    assert_eq!(cached["friends"][0]["my_receiver"], projection);
    assert_eq!(
        cached["friends"][0]["peer_receiver"]["enc_key_id"],
        "opaque-key"
    );
}

#[tokio::test]
async fn registry_snapshots_are_reused_and_invalidated_on_freshness_loss() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    service.remember_neighbor_registry(
        &json!({"access":"paid","friends":[{"friendship_id":"a".repeat(64),"permission":{"scope":"opaque"}},{"friendship_id":"b".repeat(64)}]}),
    );
    assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
    assert!(service.neighbor_registry_receiving(&"b".repeat(64)));
    assert!(service
        .neighbor_cell()
        .registry_snapshot
        .lock()
        .as_ref()
        .unwrap()["friends"][0]
        .get("permission")
        .is_none());
    for task in service.stop_neighbor_friendship_listeners(&"a".repeat(64)) {
        task.abort();
    }
    assert!(!service.neighbor_registry_receiving(&"a".repeat(64)));
    assert!(service.neighbor_registry_receiving(&"b".repeat(64)));
    service.remember_neighbor_registry(
        &json!({"access":"locked","friends":[{"friendship_id":"a".repeat(64)}]}),
    );
    assert!(!service.neighbor_registry_receiving(&"a".repeat(64)));
    service.pause_neighbor_reception();
    assert!(service.neighbor_cell().registry_snapshot.lock().is_none());
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    service.remember_neighbor_registry(&json!({"access":"paid","friends":[]}));
    assert!(service.neighbor_cell().registry_snapshot.lock().is_none());
}
