use super::*;

#[test]
fn sentinel_lifecycle_packet_names_graph_reason_and_history_command() {
    let notice = StopNotice {
        sentinel_id: "sentinel_budget".into(),
        name: "Budget watch".into(),
        creator_tab_id: "creator".into(),
        reason: "transition_budget_exhausted".into(),
    };

    assert_eq!(
            lifecycle_packet(&notice),
            "Blueprint graph 'Budget watch' ended. Reason: transition_budget_exhausted.\nNext step: domios blueprint history sentinel_budget"
        );
}

#[test]
fn sentinel_escalate_packet_includes_next_steps() {
    let root = tempfile::tempdir().unwrap();
    let mut store = super::super::store::Store::new(
        root.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let job = store
            .create_document(
                serde_json::json!(codeterm_sentinel::test_support::watch(serde_json::json!("escalation-packet-watch"),serde_json::json!({"uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"10s"}}),serde_json::json!({"with":{"question":{"lit":"state?"}},"buckets":["done"],"matches":{"done":{"lit":"done"}}}),serde_json::json!(null),serde_json::json!(null))),
                super::super::store::Owner {
                    creator_tab_id: "creator".into(),
                    team_id: None,
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                false,
            )
            .unwrap();
    let packet: serde_json::Value =
        serde_json::from_str(&escalation_packet(&job, "transition_one")).unwrap();

    assert_eq!(packet["blueprint_id"], job.id);
    assert_eq!(packet["transition_id"], "transition_one");
    assert_eq!(
        packet["next_steps"],
        serde_json::json!([
            format!("inspect: domios blueprint history {}", job.id),
            format!(
                "resolve: domios blueprint escalation resolve {} transition_one",
                job.id
            )
        ])
    );
}

#[test]
fn sentinel_stop_notice_stays_out_of_the_bell() {
    let (mux, creator_tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let notice = StopNotice {
        sentinel_id: "sentinel_budget".into(),
        name: "budget watch".into(),
        creator_tab_id: creator_tab_id.clone(),
        reason: "transition_budget_exhausted".into(),
    };

    notify_stop(&fixture.service, &notice);

    let notification_store = fixture.service.notif.lock();
    let notifications = notification_store.list(false);
    assert!(notifications.is_empty());
}
