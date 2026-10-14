use super::*;
use crate::crash::restore_report::{build_restore_report, DropReason, RestorePane};
use codeterm_types::pty::BootStage;

fn restored_tab() -> crate::multiplexer::types::Tab {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let mut tab = mux.get_tab(&tab_id).unwrap().clone();
    tab.agent_type = Some("restore-fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("current-launch".into());
    tab.boot_stage = Some(BootStage::ShellReady);
    tab.restored_from_snapshot = true;
    tab
}

fn evidence(tab: &crate::multiplexer::types::Tab) -> LivenessEvidence {
    LivenessEvidence {
        declares_agent: tab.agent_type.is_some(),
        backing: AgentBacking::Pty {
            pty_live: Some(true),
        },
        boot_stage: tab.boot_stage,
        boot_in_flight: tab.agent_booting,
        provider_exit_observed: false,
        pending_prompt: false,
    }
}

#[test]
fn report_before_readiness_waits_for_own_session_event() {
    let mut tab = restored_tab();
    let bound = vec![RestorePane {
        tab_id: tab.id.clone(),
        role: None,
        agent_type: tab.agent_type.clone(),
        agent_session_id: tab.agent_session_id.clone(),
        exited_session_id: None,
    }];
    let restored_ids = [tab.id.clone()].into_iter().collect();
    let report = |tab: &crate::multiplexer::types::Tab| {
        build_restore_report(
            &bound,
            &restored_ids,
            &[(
                tab.id.clone(),
                classify_restore_liveness(tab, evidence(tab)),
            )]
            .into_iter()
            .collect(),
        )
    };

    assert_eq!(
        classify_agent_liveness(evidence(&tab)),
        AgentLiveness::Absent
    );
    let opening = report(&tab);
    assert_eq!(opening.unobserved, 1);
    assert_eq!(opening.shell_only, 0);
    assert_eq!(
        opening.panes[0].drop_reason,
        Some(DropReason::AgentLivenessUnobserved)
    );

    tab.restored_from_snapshot = false;
    tab.boot_stage = Some(BootStage::LaunchWritten);
    assert_eq!(
        classify_agent_liveness(evidence(&tab)),
        AgentLiveness::Absent
    );
    assert_eq!(report(&tab).unobserved, 1);

    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::ProviderReady);
    assert_eq!(report(&tab).unobserved, 1);

    tab.agent_booting = false;
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.reported_session_binding_nonce = Some("foreign-launch".into());
    assert_eq!(report(&tab).unobserved, 1);

    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    let ready = report(&tab);
    assert!(ready.every_pane_has_a_live_agent());
    assert_eq!(
        ready.panes[0].pane.agent_session_id.as_deref(),
        Some("own-session")
    );
    assert_eq!(ready.panes[0].liveness, AgentLiveness::Live);
    assert_eq!(ready.panes[0].drop_reason, None);
}

#[test]
fn report_readiness_needs_current_launch_and_live_backing() {
    let mut tab = restored_tab();
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert_eq!(
        classify_restore_liveness(&tab, evidence(&tab)),
        AgentLiveness::Unobserved
    );
    tab.restored_from_snapshot = false;
    let mut dead = evidence(&tab);
    dead.backing = AgentBacking::Pty {
        pty_live: Some(false),
    };
    assert_eq!(
        classify_restore_liveness(&tab, dead),
        AgentLiveness::Unobserved
    );
    dead.provider_exit_observed = true;
    assert_eq!(classify_restore_liveness(&tab, dead), AgentLiveness::Absent);
}

#[test]
fn report_preserves_explicit_failures_and_exited_members() {
    let mut tab = restored_tab();
    tab.boot_stage = Some(BootStage::SpawnFailed);
    assert_eq!(
        classify_restore_liveness(&tab, evidence(&tab)),
        AgentLiveness::Absent
    );
    tab.agent_type = None;
    tab.boot_stage = Some(BootStage::ShellReady);
    assert_eq!(
        classify_restore_liveness(&tab, evidence(&tab)),
        AgentLiveness::Absent
    );
}

#[test]
fn report_accepts_authenticated_resume_without_guessing_from_reattachment() {
    let mut tab = restored_tab();
    tab.restored_from_snapshot = false;
    tab.boot_stage = Some(BootStage::PtyReattached);
    assert_eq!(
        classify_restore_liveness(&tab, evidence(&tab)),
        AgentLiveness::Unobserved
    );
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert_eq!(
        classify_restore_liveness(&tab, evidence(&tab)),
        AgentLiveness::Live
    );
}

#[test]
fn report_retains_plugin_session_liveness_for_chat_backends() {
    let tab = restored_tab();
    for session_open in [false, true] {
        let mut plugin = evidence(&tab);
        plugin.backing = AgentBacking::ChatBackend { session_open };
        assert_eq!(
            classify_restore_liveness(&tab, plugin),
            classify_agent_liveness(plugin)
        );
    }
}
