use std::cell::Cell;
use std::collections::BTreeMap;

use super::*;
use crate::provenance::{Classification, Scope};
use crate::store::{Owner, Store};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::{PendingPrompt, PromptOption};

fn fixture_prompt() -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: "password=prompt-secret".into(),
        options: vec![PromptOption {
            label: "Continue".into(),
            value: "https://user:pass@example.test/path?token=query-secret".into(),
        }],
        sections: None,
        multi_select: Some(false),
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn sentinel_observe_json_is_canonical_redacted_and_screen_bounded() {
    let screen = format!("{} token=screen-secret", "screen ".repeat(800));
    let first = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        false,
        None,
        Some(&screen),
    )
    .unwrap();
    let second = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        false,
        None,
        Some(&screen),
    )
    .unwrap();
    let encoded = serde_json::to_string(&first).unwrap();
    assert_eq!(encoded, serde_json::to_string(&second).unwrap());
    assert!(!encoded.contains("provider_id"));
    assert!(!encoded.contains("fixture-provider"));
    assert!(!encoded.contains("prompt-secret"));
    assert!(!encoded.contains("query-secret"));
    assert!(!encoded.contains("screen-secret"));
    assert!(first["screen_tail"].as_str().unwrap().len() <= MAX_SCREEN_BYTES);
    assert_eq!(first["prompt"]["kind"], "provider");
}

#[test]
fn sentinel_observe_source_requires_agent_in_creator_team_workspace() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let bound = mux.create_tab(&workspace).unwrap();
    let other_workspace = mux.create_workspace("Other".into(), None).id;
    let foreign = mux.create_tab(&other_workspace).unwrap();
    for id in [&bound, &foreign] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.team_id = Some("creator-team".into());
    }
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1_000),
    );
    let job = store
        .prepare_document(codeterm_sentinel::test_support::watch(serde_json::to_value::<Option<String>>(Some("observed-state-watch".into())).unwrap(),serde_json::json!({"uses":"observe","with":serde_json::json!({"kind":{"lit":"tab"},"id":{"lit":serde_json::to_value::<String>(bound.clone()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("10s".into()).unwrap()}),"paused":false,"session":false}),serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("state?".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["waiting".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(BTreeMap::new()).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(Default::default()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),serde_json::Value::Null,serde_json::to_value::<Option<String>>(None).unwrap()),
            Owner {
                creator_tab_id: creator,
                team_id: Some("creator-team".into()),
                workspace_id: workspace,
                cwd: ".".into(),
            },
            true,
        )
        .unwrap();
    assert!(validate_tab_scope(&mux, &job, &bound).is_ok());
    assert!(validate_tab_scope(&mux, &job, &foreign).is_err());
    mux.get_tab_mut(&bound).unwrap().team_id = None;
    assert!(validate_tab_scope(&mux, &job, &bound).is_err());
    mux.get_tab_mut(&bound).unwrap().team_id = Some("creator-team".into());
    mux.get_tab_mut(&bound).unwrap().agent_type = None;
    assert!(validate_tab_scope(&mux, &job, &bound).is_err());
}

#[test]
fn sentinel_direct_observation_survives_creator_joining_a_team() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("fixture-provider".into());
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(
        temp.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1_000),
    );
    let job = store
        .prepare_document(codeterm_sentinel::test_support::watch(serde_json::to_value::<Option<String>>(Some("creator-state-watch".into())).unwrap(),serde_json::json!({"uses":"observe","with":serde_json::json!({"kind":{"lit":"tab"},"id":{"lit":serde_json::to_value::<String>(creator.clone()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("10s".into()).unwrap()}),"paused":false,"session":false}),serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("state?".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["waiting".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(BTreeMap::new()).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(Default::default()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),serde_json::Value::Null,serde_json::to_value::<Option<String>>(None).unwrap()),
            Owner {
                creator_tab_id: creator.clone(),
                team_id: None,
                workspace_id: workspace,
                cwd: ".".into(),
            },
            true,
        )
        .unwrap();
    assert!(validate_tab_scope(&mux, &job, &creator).is_ok());
    mux.get_tab_mut(&creator).unwrap().team_id = Some("formed-team".into());
    assert!(validate_tab_scope(&mux, &job, &creator).is_ok());
}

#[test]
fn sentinel_removed_observation_tab_stops_with_one_creator_notice() {
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        temp.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1_000),
    );
    let job = store
        .create_document(codeterm_sentinel::test_support::watch(serde_json::to_value::<Option<String>>(Some("removed-tab-watch".into())).unwrap(),serde_json::json!({"uses":"observe","with":serde_json::json!({"kind":{"lit":"tab"},"id":{"lit":serde_json::to_value::<String>("removed-tab".into()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("10s".into()).unwrap()}),"paused":false,"session":false}),serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("state?".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["waiting".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(BTreeMap::new()).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(Default::default()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),serde_json::Value::Null,serde_json::to_value::<Option<String>>(None).unwrap()),
            Owner {
                creator_tab_id: creator.clone(),
                team_id: None,
                workspace_id: workspace,
                cwd: ".".into(),
            },
            true,
        )
        .unwrap();

    let binding = job.observe().unwrap();
    let failure = super::observe(&fixture.service, &job, binding).unwrap_err();
    assert_eq!(failure.reason(), "blueprint_observation_tab_removed");

    let store = std::sync::Arc::new(parking_lot::Mutex::new(store));
    crate::storage::commit(&store, |store| {
        store.stage_stop(&job.id, "observed_tab_removed")
    })
    .unwrap();
    let notices = store.lock().take_stop_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].creator_tab_id, creator);
    assert_eq!(notices[0].reason, "observed_tab_removed");
}

#[test]
fn sentinel_cron_run_scope_accepts_only_its_verified_hidden_automation_tab() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let run_tab = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&run_tab).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.audience = codeterm_service_core::multiplexer::types::TabAudience::UserOwnedAutomation;
    tab.hidden = true;
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(
        temp.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1_000),
    );
    let mut job = store
        .prepare_document(codeterm_sentinel::test_support::watch(serde_json::to_value::<Option<String>>(Some("cron-state-watch".into())).unwrap(),serde_json::json!({"uses":"observe","with":serde_json::json!({"kind":{"lit":"cron_run"},"id":{"lit":serde_json::to_value::<String>("cron-nightly".into()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("10s".into()).unwrap()}),"paused":false,"session":false}),serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("run state?".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["waiting".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(BTreeMap::new()).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(Default::default()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),serde_json::Value::Null,serde_json::to_value::<Option<String>>(None).unwrap()),
            Owner {
                creator_tab_id: creator,
                team_id: Some("creator-team".into()),
                workspace_id: workspace.clone(),
                cwd: ".".into(),
            },
            true,
        )
        .unwrap();
    job.run = Some(RunBinding {
        run_id: "run-current".into(),
        tab_id: run_tab.clone(),
        bound_at: 1_000,
    });

    assert!(validate_pending_cron_run_scope(&mux, &job, "cron-nightly", &run_tab).is_ok());
    assert!(validate_tab_scope(&mux, &job, &run_tab).is_ok());
    assert!(validate_pending_cron_run_scope(&mux, &job, "cron-other", &run_tab).is_err());

    let unbound_automation = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&unbound_automation).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.audience = codeterm_service_core::multiplexer::types::TabAudience::UserOwnedAutomation;
    tab.hidden = true;
    assert!(validate_tab_scope(&mux, &job, &unbound_automation).is_err());
}

#[test]
fn sentinel_observe_unchanged_tab_observation_makes_no_second_classify_call() {
    let value = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        true,
        None,
        None,
    )
    .unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut machine = super::super::machine::Machine::default();
    let calls = Cell::new(0);
    for _ in 0..2 {
        super::super::machine::evaluate(
            &mut machine,
            &bytes,
            &["waiting".into()],
            &BTreeMap::new(),
            None,
            Scope {
                sentinel_id: "watch".into(),
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
            },
            || {
                calls.set(calls.get() + 1);
                Classification::adapter(Ok(super::super::machine::Verdict {
                    bucket: "waiting".into(),
                    confidence: 0.9,
                }))
            },
        );
    }
    assert_eq!(calls.get(), 1);
}
