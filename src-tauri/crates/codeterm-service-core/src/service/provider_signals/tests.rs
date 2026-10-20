use super::*;
use codeterm_provider_signals::{AuthStatus, SessionObservations};

#[test]
fn claude_signals_auth_overrides_scrape_only_when_available() {
    let screen = TabStateClassification {
        state: TabState::WaitingOnAuth,
        confidence: 0.9,
        data: None,
        action: None,
    };
    let mut signals = ProviderSignals::default();
    assert_eq!(
        classify(&signals, Some(screen.clone())),
        Some(screen.clone())
    );
    signals.auth = Some(AuthStatus {
        logged_in: true,
        auth_method: Some("oauth".into()),
        subscription_type: Some("max".into()),
        config_directory: Some("fixture".into()),
    });
    assert!(classify(&signals, Some(screen)).is_none());
    signals.auth.as_mut().unwrap().logged_in = false;
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::WaitingOnAuth
    );
}

#[test]
fn claude_signals_transcript_errors_and_plan_drift_are_typed() {
    let mut signals = ProviderSignals::default();
    signals.observe(&SessionObservations {
        api_error: Some(ApiErrorKind::RateLimit),
        permission_mode: Some(PermissionMode::Plan),
        ..Default::default()
    });
    assert!(plan_drift(&signals));
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::RateLimited
    );
    signals.observe(&SessionObservations {
        clears_api_error: true,
        ..Default::default()
    });
    assert_eq!(signals.api_error, None);
    assert!(plan_drift(&signals));
    signals.observe(&SessionObservations {
        permission_mode: Some(PermissionMode::Default),
        ..Default::default()
    });
    assert!(!plan_drift(&signals));
}

#[test]
fn provider_access_incident_transition_notifies_once_routes_roles_and_recovers() {
    use super::incident::{incident_transition, parent_target, IncidentTransition};
    let mut signals = ProviderSignals::default();
    let mut entered = 0;
    for error in [
        ApiErrorKind::Authentication,
        ApiErrorKind::Authentication,
        ApiErrorKind::Other,
    ] {
        let before = signals.access_lost();
        signals.observe(&SessionObservations {
            api_error: Some(error),
            ..Default::default()
        });
        if incident_transition(before, signals.access_lost()) == IncidentTransition::Enter {
            entered += 1;
        }
        assert_eq!(
            classify(&signals, None).unwrap().state,
            TabState::WaitingOnAuth
        );
    }
    assert_eq!(entered, 1);
    assert_eq!(
        parent_target(true, Some("parent".into()), Some("owner".into()), "own"),
        Some("parent".into())
    );
    assert_eq!(
        parent_target(true, None, Some("owner".into()), "own"),
        Some("owner".into())
    );
    assert_eq!(
        parent_target(false, Some("parent".into()), Some("owner".into()), "own"),
        Some("parent".into())
    );
    assert_eq!(
        parent_target(false, None, Some("owner".into()), "own"),
        None
    );
    assert_eq!(parent_target(true, None, Some("own".into()), "own"), None);
    let before = signals.access_lost();
    signals.observe(&SessionObservations {
        clears_api_error: true,
        ..Default::default()
    });
    assert_eq!(
        incident_transition(before, signals.access_lost()),
        IncidentTransition::Recover
    );
    assert!(signals.access_state().is_none());
    assert!(classify(&signals, None).is_none());
    signals.observe(&SessionObservations {
        api_error: Some(ApiErrorKind::Authentication),
        ..Default::default()
    });
    assert_eq!(
        incident_transition(false, signals.access_lost()),
        IncidentTransition::Enter
    );
}

#[test]
fn provider_access_recovery_resolves_a_preexisting_condition() {
    use crate::notification::store::{
        Ask, NotificationProvenance, NotificationSeverity, NotificationStore, PushRequest,
    };
    let mut store = NotificationStore::new(10);
    let request = PushRequest {
        scope: "own".into(),
        level: crate::notification::push::NotifyLevel::ActionNeeded,
        event_key: "condition-token".into(),
        title: "opaque".into(),
        body: None,
        severity: NotificationSeverity::Error,
        provenance: NotificationProvenance {
            tab_id: Some("own".into()),
            ..Default::default()
        },
        per_scope_limit: 100,
        ask: Ask::about(
            crate::notification::dedupe::TabCondition::ProviderAccessLost,
            None,
        ),
    };
    let admission = store.add_push(request);
    let id = admission.id().to_string();
    assert_eq!(
        store.level(&id),
        Some(crate::notification::push::NotifyLevel::ActionNeeded)
    );
    assert_eq!(
        store.notification(&id).unwrap().severity,
        NotificationSeverity::Error
    );
    assert_eq!(
        store.resolve_tab_condition(
            "foreign",
            crate::notification::dedupe::TabCondition::ProviderAccessLost
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        store.resolve_tab_condition(
            "own",
            crate::notification::dedupe::TabCondition::ProviderAccessLost
        ),
        vec![id.clone()]
    );
    assert!(store.notification(&id).unwrap().resolved);
    assert!(store.notification(&id).unwrap().read);
    assert!(store
        .resolve_tab_condition(
            "own",
            crate::notification::dedupe::TabCondition::ProviderAccessLost,
        )
        .is_empty());
}

#[test]
fn provider_turn_classes_classify_without_taking_the_auth_loss_latch() {
    for (error, state, lost) in [
        (ApiErrorKind::UsageLimit, TabState::RateLimited, false),
        (ApiErrorKind::RateLimit, TabState::RateLimited, false),
        (ApiErrorKind::ServerOverloaded, TabState::Unknown, false),
        (ApiErrorKind::AuthExpiry, TabState::WaitingOnAuth, false),
        (ApiErrorKind::Authentication, TabState::WaitingOnAuth, true),
    ] {
        let mut signals = ProviderSignals::default();
        signals.observe(&SessionObservations {
            api_error: Some(error),
            ..Default::default()
        });
        assert_eq!(classify(&signals, None).unwrap().state, state);
        assert_eq!(signals.access_lost(), lost);
    }
}

#[test]
fn one_failed_turn_admits_one_incident_and_a_repeat_adds_none() {
    use crate::agent::chat_types::RichMessage;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TaskBinding;
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.task_binding = Some(TaskBinding {
        task_id: Some("task".into()),
        ..Default::default()
    });
    tab.provider_signals.api_error = Some(ApiErrorKind::Authentication);
    let rate = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("turn-429".into()),
        provider_observations: Some(SessionObservations {
            api_error: Some(ApiErrorKind::RateLimit),
            retry_after_secs: Some(12),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fresh = fresh_turn_incidents(tab, "own", &[rate.clone()]);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].worker, tab_id);
    assert_eq!(fresh[0].task.as_deref(), Some("task"));
    assert_eq!(fresh[0].provider, "fixture");
    assert_eq!(fresh[0].class, ApiErrorKind::RateLimit);
    assert_eq!(fresh[0].retry_after, Some(12));
    assert!(fresh[0].turn_stopped);
    assert!(!fresh[0].pauses_for_exhaustion);
    assert!(tab.provider_signals.access_lost());
    assert!(fresh_turn_incidents(tab, "own", &[rate]).is_empty());
    let foreign = RichMessage {
        provider_session_id: Some("foreign".into()),
        uuid: Some("turn-foreign".into()),
        provider_observations: Some(SessionObservations {
            api_error: Some(ApiErrorKind::UsageLimit),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(fresh_turn_incidents(tab, "own", &[foreign]).is_empty());
    let usage = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("turn-usage".into()),
        provider_observations: Some(SessionObservations {
            api_error: Some(ApiErrorKind::UsageLimit),
            ..Default::default()
        }),
        ..Default::default()
    };
    let paused = fresh_turn_incidents(tab, "own", &[usage]);
    assert_eq!(paused.len(), 1);
    assert!(paused[0].pauses_for_exhaustion);
    let auth = RichMessage {
        provider_session_id: Some("own".into()),
        uuid: Some("turn-auth".into()),
        provider_observations: Some(SessionObservations {
            api_error: Some(ApiErrorKind::Authentication),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(fresh_turn_incidents(tab, "own", &[auth]).is_empty());
}

#[test]
fn provider_turn_recovery_resolves_a_preexisting_condition() {
    use crate::notification::store::{
        Ask, NotificationProvenance, NotificationSeverity, NotificationStore, PushRequest,
    };
    let mut store = NotificationStore::new(10);
    let request = PushRequest {
        scope: "own".into(),
        level: crate::notification::push::NotifyLevel::Verbose,
        event_key: "turn-token".into(),
        title: "opaque".into(),
        body: None,
        severity: NotificationSeverity::Error,
        provenance: NotificationProvenance {
            tab_id: Some("own".into()),
            ..Default::default()
        },
        per_scope_limit: 100,
        ask: Ask::about(
            crate::notification::dedupe::TabCondition::ProviderTurnError,
            None,
        ),
    };
    let id = store.add_push(request).id().to_string();
    assert_eq!(
        store.resolve_tab_condition(
            "own",
            crate::notification::dedupe::TabCondition::ProviderTurnError,
        ),
        vec![id.clone()]
    );
    assert!(store.notification(&id).unwrap().resolved);
    assert!(store
        .resolve_tab_condition(
            "own",
            crate::notification::dedupe::TabCondition::ProviderAccessLost,
        )
        .is_empty());
}

#[test]
fn provider_access_records_ignore_foreign_session_errors_and_recovery() {
    use crate::agent::chat_types::RichMessage;
    let record = |session: &str, error, clears| RichMessage {
        provider_session_id: Some(session.into()),
        provider_observations: Some(SessionObservations {
            api_error: error,
            clears_api_error: clears,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut signals = ProviderSignals::default();
    observe_records(
        &mut signals,
        "own",
        &[record("foreign", Some(ApiErrorKind::Authentication), false)],
        false,
    );
    assert!(!signals.access_lost());
    observe_records(
        &mut signals,
        "own",
        &[record("own", Some(ApiErrorKind::Authentication), false)],
        false,
    );
    assert!(signals.access_lost());
    observe_records(&mut signals, "own", &[record("foreign", None, true)], false);
    assert!(signals.access_lost());
    observe_records(&mut signals, "own", &[record("own", None, true)], false);
    assert!(!signals.access_lost());
}

#[test]
fn provider_access_fermi_fixture_enters_incident_once_and_recovers() {
    use super::incident::{incident_transition, IncidentTransition};
    use crate::agent::chat_types::RichMessage;
    let fixture = include_str!(
        "../../../../../../plugins/provider-claude/tests/fixtures/access-lost/fermi.jsonl"
    );
    let mut signals = ProviderSignals::default();
    let mut entered = 0;
    let records: Vec<serde_json::Value> = fixture
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let session = "ef8d7026-7956-400b-aa88-9b7ef811f9ce";
    for record in records.iter().chain(records.iter()) {
        if record["isApiErrorMessage"] != true {
            continue;
        }
        let before = signals.access_lost();
        observe_records(
            &mut signals,
            session,
            &[RichMessage {
                provider_session_id: record["sessionId"].as_str().map(str::to_string),
                provider_observations: Some(SessionObservations {
                    api_error: Some(ApiErrorKind::from_error(record.get("error"), record)),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            false,
        );
        if incident_transition(before, signals.access_lost()) == IncidentTransition::Enter {
            entered += 1;
        }
    }
    assert_eq!(entered, 1);
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::WaitingOnAuth
    );
    let before = signals.access_lost();
    signals.observe(&SessionObservations {
        clears_api_error: true,
        ..Default::default()
    });
    assert_eq!(
        incident_transition(before, signals.access_lost()),
        IncidentTransition::Recover
    );
}

#[test]
fn structured_sqlite_error_observations_reach_only_the_claimed_session() {
    let delta: codeterm_types::agent::chat_types::StructuredChat = serde_json::from_value(serde_json::json!({
        "messages":[{"id":"error:failed","type":"system","systemSubtype":"api_error","content":"Model exo-free has been deprecated"}],
        "observations":[{"role":"assistant","provider_session_id":"own","provider_observations":{"api_error":"other"}}]
    })).unwrap();
    let mut foreign = ProviderSignals::default();
    observe_records(&mut foreign, "foreign", &delta.observations, false);
    assert_eq!(foreign.api_error, None);
    let mut own = ProviderSignals::default();
    observe_records(&mut own, "own", &delta.observations, false);
    assert_eq!(own.api_error, Some(ApiErrorKind::Other));
    assert_eq!(
        delta.messages[0].system_subtype.as_deref(),
        Some("api_error")
    );
}

fn fresh_turn_incidents(
    tab: &mut crate::multiplexer::types::Tab,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
) -> Vec<codeterm_provider_signals::ProviderTurnIncident> {
    let snapshot = super::update::SignalSnapshot::capture(tab);
    super::update::fresh_turn_incidents(&mut tab.provider_signals, &snapshot, session, records)
}
