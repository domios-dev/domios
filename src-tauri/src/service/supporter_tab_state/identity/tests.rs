use super::{supporter_policy, DecisionErrorCategory, DecisionSource};
use codeterm_types::config::{SupporterConfig, SupporterConnection};
use codeterm_types::decision::DecisionError;
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

#[test]
fn supporter_decision_error_categories_are_sanitized_into_records() {
    let cases = [
        (
            DecisionError::Transport("token=private-transport".into()),
            DecisionErrorCategory::Transport,
        ),
        (DecisionError::Http(503), DecisionErrorCategory::Http),
        (
            DecisionError::Unavailable("token=private-unavailable".into()),
            DecisionErrorCategory::Unavailable,
        ),
        (
            DecisionError::Unsupported,
            DecisionErrorCategory::Unsupported,
        ),
        (
            DecisionError::Parse("token=private-parse".into()),
            DecisionErrorCategory::Parse,
        ),
        (
            DecisionError::Parse("invalid_observation".into()),
            DecisionErrorCategory::InvalidObservation,
        ),
    ];

    for (error, category) in cases {
        let source = DecisionSource::error(&error);
        let record = super::journal("tab", 1, source, "other", None, "title");
        assert_eq!(source.as_str(), "fallback(error)");
        assert_eq!(record["error_category"], category.as_str());
        assert!(!record.to_string().contains("private-"));
    }
}

#[test]
fn supporter_decision_error_categories_leave_timeout_and_budget_distinct() {
    for (error, expected) in [
        (DecisionError::Deadline, DecisionSource::Timeout),
        (DecisionError::Budget, DecisionSource::Budget),
        (DecisionError::CreditExhausted, DecisionSource::Budget),
    ] {
        let source = DecisionSource::error(&error);
        let record = super::journal("tab", 1, source, "other", None, "title");
        assert_eq!(source, expected);
        assert!(record["error_category"].is_null());
    }
}
