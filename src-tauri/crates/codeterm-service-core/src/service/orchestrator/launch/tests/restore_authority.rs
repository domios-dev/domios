use super::super::account_supervision::{self, LaunchAccount};
use super::{BootStage, LaunchFailure, LaunchGeneration, MultiplexerState, LAUNCH_REGISTRY};
use crate::multiplexer::types::AgentRole;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::launch_readiness;
use crate::service::test_fixture::ServiceFixture;
use crate::service::ServiceContext;

const PROVIDER: &str = "fixture-restore-provider";
const ACCOUNT: &str = "fixture-shared-account";

fn save_manager(mux: &mut MultiplexerState, tab_id: &str, pty: &str, session: &str) {
    mux.link_pty(tab_id, pty.into());
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some(session.into());
    tab.session_binding_nonce = Some(format!("saved-{session}"));
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.restorable_launch.account_id = Some(ACCOUNT.into());
    tab.restored_from_snapshot = true;
    tab.role = Some(AgentRole::Manager);
    tab.team_id = Some("fixture-team".into());
}

fn arm_restored_manager(
    svc: &ServiceContext,
    tab_id: &str,
    pty: &str,
    session: &str,
) -> (LaunchAccount, LaunchGeneration) {
    assert!(svc
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .launch_session_authenticated());
    let restored = crate::service::pty_io::claim_restored_pty_boot(&svc.mux, tab_id)
        .unwrap()
        .unwrap();
    assert_eq!(restored.session_id.as_deref(), Some(session));
    assert_eq!(
        restored.launch_parameters.account_id.as_deref(),
        Some(ACCOUNT)
    );
    account_supervision::mark_tab_agent_launch_started(&svc.mux, tab_id);
    let (account, reserved_nonce) =
        account_supervision::prepare(svc, tab_id, PROVIDER, restored.session_id.as_deref())
            .unwrap();
    let mut launch = LaunchGeneration::begin(tab_id, pty.into());
    assert!(launch.emit(svc, BootStage::PtyStarting));
    assert!(launch.emit(svc, BootStage::PtySpawned));
    assert!(launch.emit(svc, BootStage::ReaderReady));
    launch
        .capture_binding_nonce(
            svc,
            None,
            None,
            AgentLaunchOrigin::RestoredSnapshot,
            restored.session_id.as_deref(),
            reserved_nonce.clone(),
        )
        .unwrap();
    assert_eq!(launch.binding_nonce, reserved_nonce);
    assert!(launch.emit(svc, BootStage::ShellReady));
    assert!(launch.emit(svc, BootStage::AgentLaunching));
    (account, launch)
}

#[test]
fn overlapping_saved_managers_each_dispatch_and_authenticate_their_original_session() {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.get_active_workspace().unwrap().id.clone();
    let peer = mux.create_tab(&workspace).unwrap();
    let managers = [
        (root.as_str(), "root-pty", "root-session"),
        (peer.as_str(), "peer-pty", "peer-session"),
    ];
    for (tab_id, pty, session) in managers {
        save_manager(&mut mux, tab_id, pty, session);
    }
    let root_tab = mux.get_tab_mut(&root).unwrap();
    root_tab.role_profile = Some("orchestrator".into());
    root_tab.can_spawn = true;
    let peer_tab = mux.get_tab_mut(&peer).unwrap();
    peer_tab.role_profile = Some("planner".into());
    peer_tab.parent_tab_id = Some(root.clone());
    peer_tab.depth = 2;
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let pending =
        managers.map(|(tab_id, pty, session)| arm_restored_manager(svc, tab_id, pty, session));
    assert_ne!(pending[0].1.binding_nonce, pending[1].1.binding_nonce);

    let mut dispatched = Vec::new();
    for ((tab_id, pty, session), (account, mut launch)) in managers.into_iter().zip(pending) {
        let attempt = launch.attempt().unwrap();
        assert_ne!(attempt.binding_nonce, format!("saved-{session}"));
        assert_eq!(
            launch_readiness::LaunchAttempt::current(&svc.mux, tab_id, pty),
            Some(attempt.clone())
        );
        account
            .write_command(svc, &launch, || {
                dispatched.push((pty, session));
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert!(launch.emit(svc, BootStage::LaunchWritten));
        assert!(!launch.reconcile_authenticated_binding(svc));
        {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.reported_session_binding_nonce = Some(attempt.binding_nonce.clone());
        }
        assert!(launch.reconcile_authenticated_binding(svc));
        assert!(launch.emit(svc, BootStage::AgentReady));
        let tab = svc.mux.read().get_tab(tab_id).unwrap().clone();
        assert_eq!(tab.agent_type.as_deref(), Some(PROVIDER));
        assert_eq!(tab.agent_session_id.as_deref(), Some(session));
        assert!(tab.launch_session_authenticated());
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        launch_readiness::retire_attempt(&attempt);
        LAUNCH_REGISTRY.lock().finish(&launch);
    }
    assert_eq!(
        dispatched,
        [("root-pty", "root-session"), ("peer-pty", "peer-session")]
    );
}

#[test]
fn changed_restore_account_refuses_dispatch_and_leaves_an_explicit_retry() {
    assert_restore_dispatch_refused(false);
}

#[test]
fn definitive_restore_refusal_supersedes_a_provisional_launch_deadline() {
    assert_restore_dispatch_refused(true);
}

fn assert_restore_dispatch_refused(deadline_elapsed: bool) {
    let (mut mux, tab_id) = MultiplexerState::new();
    save_manager(&mut mux, &tab_id, "restore-pty", "saved-session");
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let (account, launch) = arm_restored_manager(svc, &tab_id, "restore-pty", "saved-session");
    let attempt = launch.attempt().unwrap();
    if deadline_elapsed {
        svc.record_launch_output_deadline(&attempt, "readiness deadline");
        let mux = svc.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert!(tab.provisional_launch.is_some());
    }
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .account_id = Some("changed-account".into());

    let failure = account
        .write_command(svc, &launch, || {
            panic!("a changed account must block the write")
        })
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(&failure, LaunchFailure::LaunchAuthorityChanged(reason)
        if reason.starts_with("launch_account_source_changed:"))
    );
    let reason = failure.reason();
    account.fail_undispatched_command(svc, &launch, &reason);
    svc.record_launch_output_deadline(&attempt, "stale deadline");
    assert!(!LAUNCH_REGISTRY.lock().owns(&launch));
    assert!(!launch_readiness::attempt_is_armed(&attempt));
    let tab = svc.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason.as_str()));
    assert!(tab.provisional_launch.is_none());
    assert!(!tab.agent_booting);
    assert!(!tab.launch_session_authenticated());
    assert_eq!(tab.agent_type.as_deref(), Some(PROVIDER));
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    assert_eq!(
        tab.restorable_launch.account_id.as_deref(),
        Some("changed-account")
    );
    assert!(crate::service::lazy_restore::wake_eligible(&tab, true));
    assert!(!crate::service::lazy_restore::wake_on_open_eligible(&tab));
}

#[test]
fn stale_restore_rejection_preserves_the_replacement_launch() {
    let (mut mux, tab_id) = MultiplexerState::new();
    save_manager(&mut mux, &tab_id, "restore-pty", "saved-session");
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let (account, launch) = arm_restored_manager(svc, &tab_id, "restore-pty", "saved-session");
    let attempt = launch.attempt().unwrap();
    let replacement_nonce = svc.rotate_session_binding_nonce(&tab_id);
    let failure = account
        .write_command(svc, &launch, || {
            panic!("a replaced launch must block the write")
        })
        .unwrap()
        .unwrap_err();
    account.fail_undispatched_command(svc, &launch, &failure.reason());
    assert!(!launch_readiness::attempt_is_armed(&attempt));
    let tab = svc.mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(
        tab.session_binding_nonce.as_deref(),
        Some(replacement_nonce.as_str())
    );
    assert_eq!(tab.boot_stage, Some(BootStage::AgentLaunching));
    assert!(tab.boot_failure_reason.is_none());
    assert!(tab.agent_booting);
}
