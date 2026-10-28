use super::*;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::test_fixture::ServiceFixture;

fn restored_general() -> (ServiceFixture, LaunchAttempt) {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&id, "own-pty".into());
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = true;
    tab.restored_from_snapshot = true;
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("retained-general-session".into());
    tab.session_binding_nonce = Some("current-launch-nonce".into());
    tab.reported_session_binding_nonce = None;
    tab.agent_booting = true;
    apply_boot_stage(tab, BootStage::LaunchWritten, None);
    let attempt = LaunchAttempt {
        tab_id:id,pty_id:"own-pty".into(),binding_nonce:"current-launch-nonce".into(),
    };
    (ServiceFixture::new(mux),attempt)
}

fn bind(fixture: &ServiceFixture, attempt: &LaunchAttempt, nonce: &str) {
    fixture.service.mux.write().get_tab_mut(&attempt.tab_id).unwrap()
        .reported_session_binding_nonce = Some(nonce.into());
    fixture.service.reconcile_authenticated_agent_input(&attempt.tab_id);
}

fn deadline(fixture: &ServiceFixture, attempt: &LaunchAttempt) {
    fixture.service.mux.write().get_tab_mut(&attempt.tab_id).unwrap().agent_booting = false;
    fixture.service.record_launch_output_deadline(attempt,"opaque-output-deadline");
}

fn assert_ready_once(fixture: &ServiceFixture, attempt: &LaunchAttempt) {
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&attempt.tab_id).unwrap();
    assert_eq!(tab.boot_stage,Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason,None);
    assert_eq!(tab.provisional_launch,None);
    assert_eq!(tab.agent_session_id.as_deref(),Some("retained-general-session"));
    assert_eq!(tab.reported_session_binding_nonce,tab.session_binding_nonce);
    assert!(!tab.agent_booting);
    assert!(tab.neighbor_authority.is_none());
    drop(mux);
    let events = fixture.ui.0.lock();
    let ready_events = events.iter().filter(|(name,payload)| name == "tab-boot-stage"
        && payload["tab_id"] == attempt.tab_id && payload["stage"] == "agent_ready").count();
    assert_eq!(ready_events,1);
    assert_eq!(events.iter().filter(|(name,_)| name == "tab-boot-stage").last().unwrap().1["stage"],"agent_ready");
}

#[tokio::test]
async fn restored_general_binding_before_deadline_is_ready_without_another_binding_event() {
    let (fixture, attempt) = restored_general();
    assert!(fixture.service.require_neighbor_restore_grant(&attempt.tab_id,false).is_ok());
    bind(&fixture,&attempt,&attempt.binding_nonce);
    assert_eq!(fixture.service.mux.read().get_tab(&attempt.tab_id).unwrap().boot_stage,Some(BootStage::LaunchWritten));
    deadline(&fixture,&attempt);
    assert_ready_once(&fixture,&attempt);
    fixture.service.reconcile_authenticated_agent_input(&attempt.tab_id);
    assert_ready_once(&fixture,&attempt);
}

#[tokio::test]
async fn restored_general_binding_after_deadline_requires_the_current_nonce() {
    let (fixture, attempt) = restored_general();
    deadline(&fixture,&attempt);
    bind(&fixture,&attempt,"previous-launch-nonce");
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&attempt.tab_id).unwrap();
        assert_eq!(tab.boot_stage,Some(BootStage::LaunchDeadlineElapsed));
        assert!(tab.provisional_launch.is_some());
    }
    bind(&fixture,&attempt,&attempt.binding_nonce);
    assert_ready_once(&fixture,&attempt);
}

#[tokio::test]
async fn stale_deadline_attempt_cannot_promote_a_replaced_general_launch() {
    for (pty, nonce) in [("previous-pty","current-launch-nonce"),("own-pty","previous-launch-nonce")] {
        let (fixture, attempt) = restored_general();
        bind(&fixture,&attempt,&attempt.binding_nonce);
        let stale = LaunchAttempt { pty_id:pty.into(),binding_nonce:nonce.into(),..attempt.clone() };
        deadline(&fixture,&stale);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&attempt.tab_id).unwrap();
        assert_eq!(tab.boot_stage,Some(BootStage::LaunchWritten));
        assert_eq!(tab.provisional_launch,None);
        assert!(fixture.ui.0.lock().iter().all(|(name,_)| name != "tab-boot-stage"));
    }
}
