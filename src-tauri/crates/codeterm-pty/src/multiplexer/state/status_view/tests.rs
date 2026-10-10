use super::*;

#[test]
fn snapshot_agent_state_follows_reconciled_status() {
    let mut current: AgentLiveState = TabVerdict {
        state: TimelineState::Idle,
        subtype: None,
    }
    .into();
    current.status_line = Some("Previous turn".into());
    for (status, expected) in [
        (TabStatus::Working, TimelineState::Working),
        (TabStatus::Waiting, TimelineState::Question),
        (TabStatus::Idle, TimelineState::Idle),
        (TabStatus::Error, TimelineState::Idle),
        (TabStatus::Dead, TimelineState::Idle),
    ] {
        assert_eq!(
            agent_state_for_status(Some(&current), status).state,
            expected
        );
        assert_eq!(agent_state_for_status(None, status).state, expected);
        if expected != TimelineState::Idle {
            assert!(agent_state_for_status(Some(&current), status)
                .status_line
                .is_none());
        }
    }
}

#[test]
fn matching_state_preserves_provider_metadata() {
    let mut current: AgentLiveState = TabVerdict {
        state: TimelineState::Working,
        subtype: None,
    }
    .into();
    current.status_line = Some("Current turn".into());
    assert_eq!(
        agent_state_for_status(Some(&current), TabStatus::Working),
        current
    );
}

#[test]
fn snapshot_projects_agent_state_without_mutating_classifier() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "activity-pty".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );
    let mut tracker = TabStatusTracker::new();
    tracker.update_output("activity-pty", [1; 32]);
    for (status, expected) in [
        (TabStatus::Working, TimelineState::Working),
        (TabStatus::Waiting, TimelineState::Question),
        (TabStatus::Idle, TimelineState::Idle),
    ] {
        let snapshot = mux.snapshot_with_status_resolver(&tracker, |_, _| status);
        assert_eq!(snapshot.tab_statuses[&tab_id], status);
        assert_eq!(
            snapshot.tabs[&tab_id].agent_state.as_ref().unwrap().state,
            expected
        );
    }
    assert_eq!(
        mux.get_tab(&tab_id)
            .unwrap()
            .agent_state
            .as_ref()
            .unwrap()
            .state,
        TimelineState::Idle
    );
}

#[test]
fn provider_access_loss_overrides_idle_status_and_recovers() {
    use codeterm_types::agent::provider_signals::{ApiErrorKind, SessionObservations};
    use codeterm_types::pty::QuestionSubtype;
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "access-pty".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.provider_signals.observe(&SessionObservations {
        api_error: Some(ApiErrorKind::Authentication),
        ..Default::default()
    });
    assert_eq!(
        effective_tab_status(tab, Some(TabStatus::Idle)),
        TabStatus::Waiting
    );
    let tracker = TabStatusTracker::new();
    let snapshot = mux.snapshot_with_status_resolver(&tracker, |_, raw| raw);
    let state = snapshot.tabs[&tab_id].agent_state.as_ref().unwrap();
    assert_eq!(state.state, TimelineState::Question);
    assert_eq!(state.subtype, Some(QuestionSubtype::Auth));
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .provider_signals
        .observe(&SessionObservations {
            clears_api_error: true,
            ..Default::default()
        });
    assert_eq!(
        effective_tab_status(mux.get_tab(&tab_id).unwrap(), Some(TabStatus::Idle)),
        TabStatus::Idle
    );
}
