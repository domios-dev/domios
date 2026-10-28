use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[test]
fn change_agent_parses_provider_choices_and_omitted_defaults() {
    for supplied in [false, true] {
        let mut input = json!({"operation":"change_agent", "friendship_id":"a".repeat(64), "provider":"opaque-provider"});
        if supplied {
            input["reasoning_effort"] = json!("opaque-effort");
            input["service_tier"] = json!("opaque-tier");
        }
        let command: NeighborRuntimeCommand = serde_json::from_value(input).unwrap();
        let NeighborRuntimeCommand::ChangeAgent {
            reasoning_effort,
            service_tier,
            ..
        } = command
        else {
            panic!("wrong operation")
        };
        assert_eq!(
            reasoning_effort.as_deref(),
            supplied.then_some("opaque-effort")
        );
        assert_eq!(service_tier.as_deref(), supplied.then_some("opaque-tier"));
    }
}

#[tokio::test]
async fn binding_choices_survive_storage_and_permission_apply_and_reject_foreign_binding() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let old = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let choices = ConciergeOptions {
        reasoning_effort: Some("opaque-effort".into()),
        service_tier: Some("opaque-tier".into()),
    };
    let mut settings = Settings::default();
    settings.bindings.insert(friend.clone(), old.clone());
    settings.initial_contexts.insert(
        friend.clone(),
        crate::service::neighbors::storage::InitialContext {
            gateway_tab: old,
            digest: "opaque-digest".into(),
            launch_options: Some(choices.clone()),
        },
    );
    metadata.save_settings(settings).await.unwrap();
    for scope in ["opaque-one", "opaque-two"] {
        let expected = choices.clone();
        let workspace = workspace.clone();
        service
            .apply_neighbor_permission_with(
                &metadata,
                "owner",
                &friend,
                Permission {
                    scope: scope.into(),
                    ..Default::default()
                },
                service.neighbor_history_generation(),
                |_, _, _, _, _, options| async move {
                    assert_eq!(options, expected);
                    Ok(service.mux.write().create_tab(&workspace).unwrap())
                },
            )
            .await
            .unwrap();
        let saved = metadata.settings().await.unwrap();
        assert_eq!(service.neighbor_launch_options(&saved, &friend), choices);
        let context = &saved.initial_contexts[&friend];
        assert_eq!(context.gateway_tab, saved.bindings[&friend]);
    }
    let mut saved = metadata.settings().await.unwrap();
    saved.initial_contexts.get_mut(&friend).unwrap().gateway_tab = "unbound-tab".into();
    assert_eq!(
        service.neighbor_launch_options(&saved, &friend),
        ConciergeOptions::default()
    );
}

#[test]
fn unsupported_effort_or_tier_is_refused_before_launch() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_reasoning_efforts(
            "fixture",
            &["opaque-effort"],
        )
        .with_session_config_delivery(),
    ));
    let validate = |effort: Option<&str>, tier: Option<&str>| {
        fixture.service.validate_neighbor_launch_options(
            "fixture",
            None,
            &ConciergeOptions {
                reasoning_effort: effort.map(str::to_owned),
                service_tier: tier.map(str::to_owned),
            },
        )
    };
    assert!(validate(None, None).is_ok());
    assert!(validate(Some("opaque-effort"), None).is_ok());
    assert!(validate(Some("foreign-effort"), None).is_err());
    assert!(validate(None, Some("foreign-tier")).is_err());
}
