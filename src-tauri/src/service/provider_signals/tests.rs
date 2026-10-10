use super::*;
use codeterm_types::agent::provider_signals::{AuthStatus, SessionObservations};

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
        clears_api_error: false,
    });
    assert!(plan_drift(&signals));
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::RateLimited
    );
    signals.observe(&SessionObservations {
        api_error: None,
        permission_mode: None,
        clears_api_error: true,
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
    use super::incident::{incident_transition, intent, parent_target, IncidentTransition};
    use crate::notification::push::NotifyLevel;
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
        intent("own", "launch").level,
        Some(NotifyLevel::ActionNeeded)
    );
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
fn provider_access_recovery_resolves_the_action_needed_notification() {
    use crate::notification::store::{
        Ask, NotificationProvenance, NotificationSeverity, NotificationStore, PushRequest,
    };
    let mut store = NotificationStore::new(10);
    let intent = super::incident::intent("own", "launch");
    let request = PushRequest {
        scope: "own".into(),
        level: intent.level.unwrap(),
        event_key: intent.event_key.unwrap(),
        title: "opaque".into(),
        body: None,
        severity: NotificationSeverity::Error,
        provenance: NotificationProvenance {
            tab_id: Some("own".into()),
            ..Default::default()
        },
        per_scope_limit: 100,
        ask: Ask::default(),
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
    store.set_condition(
        &id,
        crate::notification::dedupe::TabCondition::ProviderAccessLost,
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
    assert!(!store.resolve_id(&id));
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
    let fixture =
        include_str!("../../../../plugins/provider-claude/tests/fixtures/access-lost/fermi.jsonl");
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
