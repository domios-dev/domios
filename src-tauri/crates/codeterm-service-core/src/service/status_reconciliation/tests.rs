use super::*;
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::plugin::agent_claude::ClaudeProvider;
use codeterm_types::pty::{BootStage, TabStatusTracker, TabVerdict, TimelineState};
use std::sync::Arc;

#[test]
fn snapshot_and_live_projection_agree_for_ready_prompt_repaint() {
    let (mut state, _root) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&workspace_id).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-repaint".into());
        tab.agent_type = Some("claude".into());
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.agent_state = Some(
            TabVerdict {
                state: TimelineState::Idle,
                subtype: None,
            }
            .into(),
        );
    }

    let mux: MuxState = Arc::new(TimedRwLock::new("status-reconciliation-test", state));
    let mut raw_tracker = TabStatusTracker::new();
    raw_tracker.update_output("pty-repaint", [1u8; 32]);
    let tracker: AgentTrackerState =
        Arc::new(TimedMutex::new("status-reconciliation-test", raw_tracker));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(ClaudeProvider::new()));

    let live_status = {
        let mux = mux.read();
        reconcile_tab_status(
            &registry.state_snapshot(),
            mux.get_tab(&tab_id).unwrap(),
            TabStatus::Working,
        )
    };
    let snapshot = snapshot_with_reconciled_status(&mux, &tracker, &registry);

    assert_eq!(live_status, TabStatus::Idle);
    assert_eq!(snapshot.tab_statuses.get(&tab_id), Some(&live_status));
}

#[test]
fn silent_running_shell_command_stays_working_through_idle_poll() {
    use crate::multiplexer::types::{ShellActivity, ShellCommandState, TabBacking, TabType};
    use std::time::Duration;

    let (mut state, _root) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&workspace_id).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.tab_type = TabType::Terminal;
        tab.backing = TabBacking::Pty;
        tab.pty_id = Some("pty-sleep".into());
        assert!(
            crate::agent::shell_command_state::apply_shell_command_state(
                tab,
                "pty-sleep",
                ShellCommandState {
                    activity: ShellActivity::Running,
                    last_exit_code: None,
                },
            )
        );
    }
    let mux: MuxState = Arc::new(TimedRwLock::new("silent-command-test", state));
    let mut raw_tracker = TabStatusTracker::new();
    raw_tracker.update_output("pty-sleep", [1u8; 32]);
    raw_tracker.check_idle("pty-sleep", Duration::ZERO);
    assert_eq!(raw_tracker.get_status("pty-sleep"), TabStatus::Idle);
    let tracker: AgentTrackerState = Arc::new(TimedMutex::new("silent-command-test", raw_tracker));
    let registry = AgentRegistry::new();

    let live = |mux: &MuxState| {
        reconcile_tab_status(
            &registry.state_snapshot(),
            mux.read().get_tab(&tab_id).unwrap(),
            TabStatus::Idle,
        )
    };
    assert_eq!(live(&mux), TabStatus::Working);
    let snapshot = snapshot_with_reconciled_status(&mux, &tracker, &registry);
    assert_eq!(
        snapshot.tab_statuses.get(&tab_id),
        Some(&TabStatus::Working)
    );

    {
        let mut state = mux.write();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        crate::agent::shell_command_state::apply_shell_command_state(
            tab,
            "pty-sleep",
            ShellCommandState {
                activity: ShellActivity::Idle,
                last_exit_code: Some(0),
            },
        );
    }
    assert_eq!(live(&mux), TabStatus::Idle);
    let snapshot = snapshot_with_reconciled_status(&mux, &tracker, &registry);
    assert_eq!(snapshot.tab_statuses.get(&tab_id), Some(&TabStatus::Idle));
}

#[test]
fn authoritative_snapshot_refusal_is_immediate_while_lifecycle_owns_mux() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (state, _) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("authoritative-snapshot-test", state));
    let tracker: AgentTrackerState = Arc::new(TimedMutex::new(
        "authoritative-snapshot-test",
        TabStatusTracker::new(),
    ));
    let registry = AgentRegistry::new();
    let _lifecycle_owner = mux.write();
    let (done_tx, done_rx) = mpsc::channel();
    let worker_mux = mux.clone();
    let worker_tracker = tracker.clone();
    let worker = std::thread::spawn(move || {
        done_tx
            .send(
                try_snapshot_with_reconciled_status(&worker_mux, &worker_tracker, &registry)
                    .is_none(),
            )
            .unwrap();
    });

    assert!(done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("an authoritative snapshot refusal must not wait on the mux lock"));
    worker.join().unwrap();
}

#[test]
fn snapshot_release_wait_completes_only_after_lifecycle_owner_releases() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (state, _) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("snapshot-release-wake-test", state));
    let tracker: AgentTrackerState = Arc::new(TimedMutex::new(
        "snapshot-release-wake-test",
        TabStatusTracker::new(),
    ));
    let registry = AgentRegistry::new();
    let lifecycle_owner = mux.write();
    let (started_tx, started_rx) = mpsc::channel();
    let (readable_tx, readable_rx) = mpsc::channel();
    let waiter_mux = mux.clone();
    let waiter_tracker = tracker.clone();
    let waiter = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        wait_until_snapshot_readable(&waiter_mux, &waiter_tracker);
        readable_tx.send(()).unwrap();
    });

    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        readable_rx.try_recv().is_err(),
        "an owned mux must hold the release wake"
    );
    drop(lifecycle_owner);
    readable_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("releasing the lifecycle owner must wake the waiter");
    waiter.join().unwrap();
    assert!(try_snapshot_with_reconciled_status(&mux, &tracker, &registry).is_some());
}
