use crate::service::{neighbors::NeighborRuntimeCommand, test_fixture::ServiceFixture};
use codeterm_neighbors::Error;

#[tokio::test]
async fn disabled_channel_refuses_mutations_before_network_and_keeps_configuration() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    assert!(service.require_neighbors_enabled().is_ok());
    let defaults: crate::config::schema::AppConfig =
        serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(defaults.neighbors.enabled);
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    service.neighbors_config_changed();
    assert_eq!(service.require_neighbors_enabled(), Err(Error::Disabled));
    for input in [
        serde_json::json!({"operation":"send","friendship_id":"f","kind":"question","body":"opaque","in_reply_to":null}),
        serde_json::json!({"operation":"bind","friendship_id":"f","gateway_tab":"tab"}),
        serde_json::json!({"operation":"set_permission","friendship_id":"f","permission":{"allowed":true,"scope":"opaque","allow_tab_requests":false}}),
    ] {
        let command: NeighborRuntimeCommand = serde_json::from_value(input).unwrap();
        assert_eq!(
            service
                .neighbor_runtime(command, None, None)
                .await
                .unwrap_err(),
            Error::Disabled.to_string()
        );
    }
    let saved = serde_json::to_value(service.configuration()).unwrap();
    let restored: crate::config::schema::AppConfig = serde_json::from_value(saved).unwrap();
    assert!(!restored.neighbors.enabled);
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service.neighbors_config_changed();
    assert!(service.require_neighbors_enabled().is_ok());
    let events = fixture.ui.0.lock();
    let toggles: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "neighbors-state-changed")
        .map(|(_, data)| data["enabled"].clone())
        .collect();
    assert_eq!(toggles, [serde_json::json!(false), serde_json::json!(true)]);
}
