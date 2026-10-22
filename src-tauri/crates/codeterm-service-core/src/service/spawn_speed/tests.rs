use super::*;

#[test]
fn launch_tiers_follow_each_bundled_manifest() {
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut providers = 0;
    let mut declared = 0;
    for entry in std::fs::read_dir(plugins).unwrap() {
        let path = entry.unwrap().path().join("plugin.json");
        if !path.is_file() {
            continue;
        }
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let Some(runtime) = manifest.pointer("/capabilities/agentRuntime") else {
            continue;
        };
        let tiers: Vec<RuntimeServiceTier> = serde_json::from_value(
            runtime
                .get("serviceTiers")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .unwrap();
        providers += 1;
        for tier in &tiers {
            assert_eq!(
                requested_service_tier(&tiers, &tier.id)
                    .unwrap()
                    .id
                    .as_deref(),
                Some(tier.id.as_str())
            );
            declared += 1;
        }
        assert!(requested_service_tier(&tiers, "opaque-undeclared-tier").is_err());
        assert_eq!(
            requested_service_tier(&tiers, "default").is_ok(),
            !tiers.is_empty()
        );
    }
    assert!(providers > 0);
    assert!(declared > 0);
}

#[test]
fn arbitrary_provider_tier_ids_are_preserved() {
    let tiers = vec![RuntimeServiceTier {
        id: "opaque-speed".into(),
        ..Default::default()
    }];
    assert_eq!(
        requested_service_tier(&tiers, "opaque-speed")
            .unwrap()
            .id
            .as_deref(),
        Some("opaque-speed")
    );
    assert!(requested_service_tier(&tiers, "priority").is_err());
    assert!(requested_service_tier(&[], "default").is_err());
}

#[test]
fn launch_spec_round_trip_preserves_speed() {
    let params = crate::service::SpawnAgentParams {
        agent: Some("opaque-provider".into()),
        speed: Some("opaque-speed".into()),
        ..Default::default()
    };
    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(params);
    let restored = spec.to_agent_params().unwrap();
    assert_eq!(restored.speed.as_deref(), Some("opaque-speed"));
}
