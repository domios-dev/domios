use super::*;

/// Unknown is retried rather than settled: a reader refresh runs inside the TTL.
#[tokio::test]
async fn reader_refresh_retries_an_unknown_verdict_inside_the_ttl() {
    let id = "test-reader-refresh-unknown";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    put_live(id, vec![unknown_model("fable", "Fable")], None);
    assert!(has_unknown_verdict(id));
    assert!(!is_stale(id, DEFAULT_TTL, SystemTime::now()));

    let provider: Arc<dyn AgentProvider> = Arc::new(DiscoverStub {
        id: id.to_string(),
        default: Some("fable".into()),
        discovery: Some(vec![ModelInfo {
            default_reasoning_effort: None,
            id: "fable".into(),
            display_name: "Fable".into(),
            description: None,
            available: Some(true),
            availability: Some(ModelAvailability::Available),
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        }]),
    });
    let refreshed = refresh_for_reader(provider).await;

    assert!(refreshed.is_some(), "an unknown verdict must be retried");
    assert!(!has_unknown_verdict(id));
    assert_eq!(
        resolve_models(id)[0].availability(),
        ModelAvailability::Available
    );
    cleanup(id);
}

/// Settled snapshots retain the TTL gate for optional live discovery.
#[tokio::test]
async fn reader_refresh_is_ttl_gated_once_every_verdict_is_settled() {
    let id = "test-reader-refresh-ttl-gate";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    put_live(
        id,
        vec![ModelInfo {
            default_reasoning_effort: None,
            id: "fable".into(),
            display_name: "Fable".into(),
            description: None,
            available: Some(true),
            availability: Some(ModelAvailability::Available),
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        }],
        Some("fable".into()),
    );

    let provider: Arc<dyn AgentProvider> = Arc::new(DiscoverStub {
        id: id.to_string(),
        default: Some("fable".into()),
        discovery: Some(vec![model("other", "Other")]),
    });
    assert!(refresh_for_reader(provider).await.is_none());
    cleanup(id);
}
