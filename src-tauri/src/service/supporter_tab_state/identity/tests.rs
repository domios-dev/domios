use super::supporter_policy;
use codeterm_types::config::{SupporterConfig, SupporterConnection};
use std::collections::BTreeMap;

#[test]
fn supporter_connections_observation_policy_tracks_only_active_connection_settings() {
    let mut config = SupporterConfig {
        adapter: Some("active".into()),
        connections: BTreeMap::from([
            (
                "active".into(),
                SupporterConnection {
                    model: Some("one".into()),
                    endpoint: Some("http://localhost:1234".into()),
                },
            ),
            ("saved".into(), SupporterConnection::default()),
        ]),
        ..Default::default()
    };
    let initial = supporter_policy(&config);
    config.connections.get_mut("saved").unwrap().model = Some("other".into());
    assert_eq!(supporter_policy(&config), initial);
    config.connections.get_mut("active").unwrap().model = Some("two".into());
    assert_ne!(supporter_policy(&config), initial);
    let changed_model = supporter_policy(&config);
    config.connections.get_mut("active").unwrap().endpoint = None;
    assert_ne!(supporter_policy(&config), changed_model);
    config.adapter = Some("saved".into());
    assert_ne!(supporter_policy(&config), initial);
    config.adapter = None;
    let disconnected = supporter_policy(&config);
    config.connections.get_mut("active").unwrap().model = Some("three".into());
    assert_eq!(supporter_policy(&config), disconnected);
    config.connections.clear();
    assert_eq!(supporter_policy(&config), disconnected);
    assert_ne!(disconnected, initial);
}
