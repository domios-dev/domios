use super::*;
use crate::agent::live_state::observe_tab_liveness;
use crate::multiplexer::state::MultiplexerState;
use crate::service::process_tree::{ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeKey};
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::{classify_agent_liveness, AgentBacking, AgentLiveness, LivenessEvidence};
use std::collections::HashSet;

const SHELL_STAGES: [Option<BootStage>; 6] = [
    None,
    Some(BootStage::PtyStarting),
    Some(BootStage::PtySpawned),
    Some(BootStage::ReaderReady),
    Some(BootStage::ShellReady),
    Some(BootStage::PtyReattached),
];

/// The incident shape: the provider was relaunched by hand in the tab's
/// surviving shell and echoed this PTY's nonce with its own session.
fn relaunched_in_shell(stage: Option<BootStage>) -> Tab {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-shell".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = stage;
    tab.agent_booting = false;
    tab.agent_type = Some("provider-fixture".into());
    tab.agent_session_id = Some("session-resumed".into());
    tab.session_binding_nonce = Some("nonce-pty".into());
    tab.reported_session_binding_nonce = Some("nonce-pty".into());
    tab.clone()
}

fn liveness(tab: &Tab) -> AgentLiveness {
    classify_agent_liveness(LivenessEvidence {
        declares_agent: tab.agent_type.is_some(),
        backing: AgentBacking::Pty {
            pty_live: Some(true),
        },
        boot_stage: tab.boot_stage,
        boot_in_flight: tab.agent_booting,
        provider_exit_observed: tab.exit_suspected_at.is_some(),
        pending_prompt: tab.pending_prompt.is_some(),
    })
}

#[test]
fn agent_liveness_verified_in_tab_relaunch_becomes_observed_ready() {
    for stage in SHELL_STAGES {
        let mut tab = relaunched_in_shell(stage);
        assert_ne!(liveness(&tab), AgentLiveness::Live, "{stage:?}");
        assert!(binding_observes_in_tab_launch(&tab), "{stage:?}");

        apply_boot_stage(&mut tab, BootStage::AgentReady, None);

        assert_eq!(liveness(&tab), AgentLiveness::Live, "{stage:?}");
        assert!(!binding_observes_in_tab_launch(&tab), "ready is settled");
    }
}

#[test]
fn agent_liveness_owned_launch_in_flight_keeps_its_own_seal() {
    for stage in SHELL_STAGES {
        let mut tab = relaunched_in_shell(stage);
        tab.agent_booting = true;
        assert!(!binding_observes_in_tab_launch(&tab), "{stage:?}");
    }
}

#[test]
fn agent_liveness_unverified_binding_never_promotes() {
    let mut unreported = relaunched_in_shell(Some(BootStage::ShellReady));
    unreported.reported_session_binding_nonce = None;
    let mut foreign = relaunched_in_shell(Some(BootStage::ShellReady));
    foreign.reported_session_binding_nonce = Some("nonce-other".into());
    let mut retired = relaunched_in_shell(Some(BootStage::ShellReady));
    retired.session_binding_nonce = None;
    let mut unbound = relaunched_in_shell(Some(BootStage::ShellReady));
    unbound.agent_session_id = None;
    let mut unnamed = relaunched_in_shell(Some(BootStage::ShellReady));
    unnamed.agent_type = None;

    for tab in [unreported, foreign, retired, unbound, unnamed] {
        assert!(!binding_observes_in_tab_launch(&tab));
    }
}

#[test]
fn agent_liveness_tab_without_a_pty_is_not_promoted() {
    let mut tab = relaunched_in_shell(Some(BootStage::ShellReady));
    tab.pty_id = None;
    assert!(!binding_observes_in_tab_launch(&tab));
}

/// Launch, provisional and settled stages already have an owner and a rule;
/// this observation never overrides them.
#[test]
fn agent_liveness_launch_owned_stages_are_left_to_their_owner() {
    for stage in [
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::PreparingRoleInstructions,
        BootStage::DeliveringInstructions,
        BootStage::AgentReady,
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
        BootStage::SpawnFailed,
    ] {
        assert!(
            !binding_observes_in_tab_launch(&relaunched_in_shell(Some(stage))),
            "{stage:?}"
        );
    }
}

fn service_with_live_shell(stage: BootStage) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-shell".into());
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(stage);
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("session-resumed".into());
        tab.session_binding_nonce = Some("nonce-pty".into());
        tab.reported_session_binding_nonce = Some("nonce-pty".into());
        tab.shell_integration_nonce = Some("shell-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("claude").with_launch_binary("claude"),
    ));
    fixture
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some("pty-shell".into()),
            None,
            None,
            80,
            24,
            Default::default(),
        )
        .expect("spawn live backing PTY");
    (fixture, tab_id)
}

fn provider_tree_key(fixture: &ServiceFixture, tab_id: &str) -> ProviderTreeKey {
    fixture
        .service
        .provider_tree_key_for_tab(tab_id, "pty-shell")
        .expect("tab provider tree key")
}

fn reconcile_and_wait_for_provider_tree(
    fixture: &ServiceFixture,
    tab_id: &str,
    key: &ProviderTreeKey,
) -> crate::service::process_tree::ProviderTreeObservation {
    let (sender, receiver) = std::sync::mpsc::channel();
    fixture
        .service
        .provider_tree_witnesses
        .report_next_observation(key, sender);
    fixture
        .service
        .reconcile_boot_stage_with_session_binding(tab_id);
    receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("provider readiness tree snapshot completes off the runtime")
}

fn observed_provider_process() -> ProviderProcessTreeSnapshot {
    ProviderProcessTreeSnapshot::Present(HashSet::from([ProcessIdentity {
        pid: 77,
        start_token: "provider-start".into(),
    }]))
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_contract_verified_in_tab_relaunch_is_observed_live_and_ready() {
    let (fixture, tab_id) = service_with_live_shell(BootStage::ShellReady);
    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Absent
    );

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .boot_stage,
        Some(BootStage::AgentReady)
    );
    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Live
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn provider_tree_live_readiness_observation_is_seen_before_later_absence() {
    let (fixture, tab_id) = service_with_live_shell(BootStage::ShellReady);
    let key = provider_tree_key(&fixture, &tab_id);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, observed_provider_process());

    let observed = reconcile_and_wait_for_provider_tree(&fixture, &tab_id, &key);
    assert_eq!(
        observed.state(),
        crate::service::process_tree::ProviderTreeState::Present
    );
    assert!(fixture.service.provider_tree_witnesses.has_seen(&key));
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);
    let absent = fixture
        .service
        .observe_provider_process_tree(&tab_id, "pty-shell")
        .expect("the ready tab has a provider process tree");
    assert_eq!(
        absent.state(),
        crate::service::process_tree::ProviderTreeState::GoneAfterSeen
    );
    assert_eq!(absent.evidence(), "provider_seen_then_absent_snapshot");
}

/// A provider observed under this tab's shell and then absent from one
/// finished-marker snapshot proves exit, so the tab no longer claims it.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_contract_in_tab_relaunch_exit_is_observed_after_promotion() {
    let (fixture, tab_id) = service_with_live_shell(BootStage::ShellReady);
    let key = provider_tree_key(&fixture, &tab_id);
    let _ = reconcile_and_wait_for_provider_tree(&fixture, &tab_id, &key);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, observed_provider_process());
    assert!(fixture
        .service
        .observe_provider_process_tree(&tab_id, "pty-shell")
        .is_some());
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, ProviderProcessTreeSnapshot::Absent);

    fixture
        .service
        .process_pty_output("pty-shell", b"\x1b]633;E;;shell-nonce\x07\x1b]633;D\x07");

    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Absent
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.agent_type.is_none());
    assert_eq!(tab.boot_stage, Some(BootStage::ShellReady));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_contract_in_tab_relaunch_finished_marker_keeps_live_provider_live() {
    let (fixture, tab_id) = service_with_live_shell(BootStage::ShellReady);
    let key = provider_tree_key(&fixture, &tab_id);
    let _ = reconcile_and_wait_for_provider_tree(&fixture, &tab_id, &key);
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, observed_provider_process());
    assert!(fixture
        .service
        .observe_provider_process_tree(&tab_id, "pty-shell")
        .is_some());
    fixture
        .service
        .provider_tree_witnesses
        .script_snapshot(&key, observed_provider_process());

    fixture
        .service
        .process_pty_output("pty-shell", b"\x1b]633;E;;shell-nonce\x07\x1b]633;D\x07");

    assert_eq!(
        observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Live
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn restore_contract_verified_binding_on_a_dead_pty_is_not_promoted() {
    let (fixture, tab_id) = service_with_live_shell(BootStage::ShellReady);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("pty-gone".into());

    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);

    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .boot_stage,
        Some(BootStage::ShellReady)
    );
}
