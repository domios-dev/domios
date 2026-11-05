use crate::crash::lifecycle::ShutdownReason;
use crate::multiplexer::types::AgentRole;
use crate::pty::manager::PtyManager;
use codeterm_pty::pty::screen_state::ScreenState;
use codeterm_util::lock_hold::TimedMutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::service::shell_exec::{ExecCompletion, ExecWaiter};
use codeterm_pty::pty::manager::ChildExit;

/// The stream ended while the exit was still pending; the exit recorded
/// afterwards is the one published, whichever thread runs first.
#[tokio::test]
async fn shell_exit_after_stream_end_publishes_the_later_recorded_code() {
    let pty_id = "pty-exit-after-stream-end";
    let waiter = ExecWaiter::register(pty_id);
    let child_exit = Arc::new(ChildExit::default());

    super::publish_shell_exit_from(pty_id, Some(child_exit.clone()), Duration::from_secs(30));
    assert!(child_exit.record(Some(3)));

    assert_eq!(
        waiter.wait(Duration::from_secs(30)).await,
        Some(ExecCompletion::ShellExited { exit_code: Some(3) })
    );
}

#[tokio::test]
async fn shell_exit_publishes_unknown_when_the_exit_cannot_resolve() {
    let unrecorded = "pty-exit-deadline";
    let deadline_waiter = ExecWaiter::register(unrecorded);
    super::publish_shell_exit_from(
        unrecorded,
        Some(Arc::new(ChildExit::default())),
        Duration::ZERO,
    );
    assert_eq!(
        deadline_waiter.wait(Duration::from_secs(30)).await,
        Some(ExecCompletion::ShellExited { exit_code: None })
    );

    let unobservable = "pty-exit-unobservable";
    let unobservable_waiter = ExecWaiter::register(unobservable);
    let child_exit = Arc::new(ChildExit::default());
    child_exit.mark_unobservable();
    super::publish_shell_exit_from(unobservable, Some(child_exit), Duration::from_secs(30));
    assert_eq!(
        unobservable_waiter.wait(Duration::ZERO).await,
        Some(ExecCompletion::ShellExited { exit_code: None }),
        "an unobservable exit publishes at once"
    );
}

#[test]
fn child_exit_is_recorded_exactly_once() {
    let child_exit = ChildExit::default();
    assert_eq!(child_exit.resolved(), None);
    assert!(child_exit.record(Some(3)));
    assert!(!child_exit.record(Some(4)));
    child_exit.mark_unobservable();
    assert_eq!(child_exit.wait_resolved(Duration::ZERO), Some(Some(3)));
}

#[test]
fn update_owned_eof_preserves_tab_authority() {
    for cause in [
        ShutdownReason::Update,
        ShutdownReason::Signal,
        ShutdownReason::UserExit,
        ShutdownReason::Restart,
    ] {
        assert!(super::preserve_tab_authority(Some(cause)));
    }
    assert!(!super::preserve_tab_authority(None));
}

fn pty_state() -> crate::PtyState {
    Arc::new(TimedMutex::new("pty", PtyManager::new()))
}

fn spawn_generation(pty: &crate::PtyState, pty_id: &str) -> u64 {
    let (_, session) = {
        let gate = pty.lock().spawn_gate();
        crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new())
            .map(|built| pty.lock().register_session(Some(pty_id.to_string()), built))
    }
    .expect("spawn test PTY");
    session.generation
}

fn replace_generation(pty: &crate::PtyState, pty_id: &str) -> u64 {
    pty.lock().kill(pty_id).expect("remove replaced PTY");
    spawn_generation(pty, pty_id)
}

fn bind_generation(screen_state: &crate::ScreenStateHandle, pty_id: &str, generation: u64) {
    assert!(screen_state.bind_pty_generation(pty_id, generation));
}

#[test]
fn device_attributes_query_is_answered_without_frontend_input() {
    assert_eq!(
        terminal_query_replies(b"\x1b[c", false),
        vec![b"\x1b[?1;2c".as_slice()]
    );
}

#[test]
fn status_and_cursor_queries_are_answered_after_frontend_input_attaches() {
    assert_eq!(
        terminal_query_replies(b"\x1b[5n\x1b[6n", false),
        vec![b"\x1b[0n".as_slice(), b"\x1b[1;1R".as_slice()]
    );
}

#[test]
fn terminal_queries_are_answered_for_headless_tabs() {
    assert_eq!(
        terminal_query_replies(b"\x1b[c\x1b[6n", true),
        vec![b"\x1b[?1;2c".as_slice(), b"\x1b[1;1R".as_slice()]
    );
}

#[test]
fn pty_death_screen_teardown_closes_armed_waiter() {
    let pty = pty_state();
    let pty_generation = spawn_generation(&pty, "pty-1");
    let screen_state = Arc::new(ScreenState::new());
    bind_generation(&screen_state, "pty-1", pty_generation);
    let signal = screen_state.subscribe_screen_changes("pty-1");
    let generation = signal.generation();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let waiter_signal = signal.clone();
    let waiter = std::thread::spawn(move || {
        done_tx
            .send(waiter_signal.wait_after(generation, Duration::from_secs(1)))
            .unwrap();
    });

    assert!(super::handle_pty_screen_death(
        &pty,
        &screen_state,
        "pty-1",
        pty_generation,
    ));

    assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    waiter.join().unwrap();
    pty.lock().kill("pty-1").expect("cleanup PTY");
}

#[test]
fn reused_current_pty_eof_tears_down_through_global_seam() {
    let pty = pty_state();
    let generation = spawn_generation(&pty, "pty-1");
    let screen_state = Arc::new(ScreenState::new());
    bind_generation(&screen_state, "pty-1", generation);
    screen_state.process("pty-1", b"current session output");

    assert!(super::handle_pty_screen_death(
        &pty,
        &screen_state,
        "pty-1",
        generation,
    ));
    assert!(screen_state.get_screen_text("pty-1").is_none());
    pty.lock().kill("pty-1").expect("cleanup PTY");
}

#[test]
fn replaced_old_pty_eof_does_not_teardown_replacement_through_global_seam() {
    let pty = pty_state();
    let old_generation = spawn_generation(&pty, "pty-1");
    let screen_state = Arc::new(ScreenState::new());
    bind_generation(&screen_state, "pty-1", old_generation);
    let replacement_generation = replace_generation(&pty, "pty-1");
    bind_generation(&screen_state, "pty-1", replacement_generation);
    screen_state.process("pty-1", b"replacement session output");

    assert!(!super::handle_pty_screen_death(
        &pty,
        &screen_state,
        "pty-1",
        old_generation,
    ));
    assert!(pty
        .lock()
        .session_generation_matches("pty-1", replacement_generation));
    assert!(screen_state.get_screen_text("pty-1").is_some());
    pty.lock().kill("pty-1").expect("cleanup PTY");
}

#[test]
fn reader_termination_after_replacement_preserves_new_screen_state() {
    let pty = pty_state();
    let old_generation = spawn_generation(&pty, "pty-1");
    let screen_state = Arc::new(ScreenState::new());
    bind_generation(&screen_state, "pty-1", old_generation);
    let teardown_pty = pty.clone();
    let teardown_screen = screen_state.clone();
    let (validated_tx, validated_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let old_reader = std::thread::spawn(move || {
        result_tx
            .send(super::handle_pty_screen_death_after_validation(
                &teardown_pty,
                &teardown_screen,
                "pty-1",
                old_generation,
                || {
                    validated_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                },
            ))
            .unwrap();
    });

    validated_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let replacement_generation = replace_generation(&pty, "pty-1");
    bind_generation(&screen_state, "pty-1", replacement_generation);
    screen_state.process("pty-1", b"replacement session output");
    resume_tx.send(()).unwrap();

    assert!(!result_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    old_reader.join().unwrap();
    assert!(pty
        .lock()
        .session_generation_matches("pty-1", replacement_generation));
    assert!(screen_state.get_screen_text("pty-1").is_some());
    pty.lock().kill("pty-1").expect("cleanup PTY");
}

const ROLE_PTY: &str = "pty-role";

fn role_pty_fixture(
    role: AgentRole,
) -> (crate::service::test_fixture::ServiceFixture, String, u64) {
    let (mut mux, orch) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let tab = mux.create_tab(&workspace_id).unwrap();
    match role {
        AgentRole::Manager => {
            mux.set_tab_role_kind(&tab, AgentRole::Manager, Some("planner"), Some(&orch))
                .expect("attach manager");
        }
        _ => {
            mux.set_tab_role(&tab, role, Some(&orch))
                .expect("attach role tab");
        }
    }
    // Bind through the real API so `pty_to_tab` resolves the death to this tab —
    // a raw `tab.pty_id` write leaves the index empty and the sweep finds no owner.
    assert_eq!(mux.link_pty(&tab, ROLE_PTY.to_string()), None);
    assert_eq!(mux.tab_id_for_pty(ROLE_PTY), Some(tab.clone()));
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let generation = {
        let (_, session) = {
            let gate = fixture.service.pty.lock().spawn_gate();
            crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new()).map(
                |built| {
                    fixture
                        .service
                        .pty
                        .lock()
                        .register_session(Some(ROLE_PTY.to_string()), built)
                },
            )
        }
        .expect("spawn test PTY");
        session.generation
    };
    assert!(fixture
        .service
        .screen_state
        .bind_pty_generation(ROLE_PTY, generation));
    (fixture, tab, generation)
}

fn assert_shutdown_death_preserves_role_tabs(cause: ShutdownReason) {
    for role in [AgentRole::Worker, AgentRole::Manager] {
        let (fixture, tab, generation) = role_pty_fixture(role);
        let outcome =
            super::handle_pty_death_with_cause(&fixture.service, ROLE_PTY, generation, Some(cause));
        assert_eq!(
            outcome,
            super::PtyDeathOutcome::PreservedForShutdown,
            "{role:?} death during {cause:?} shutdown must take the suppression branch"
        );
        assert!(
            fixture.service.mux.read().get_tab(&tab).is_some(),
            "{role:?} tab must survive a {cause:?} shutdown PTY death"
        );
        fixture
            .service
            .pty
            .lock()
            .kill(ROLE_PTY)
            .expect("cleanup PTY");
    }
}

#[test]
fn updater_shutdown_pty_death_does_not_prune_role_tabs() {
    assert_shutdown_death_preserves_role_tabs(ShutdownReason::Update);
}

#[test]
fn signal_shutdown_pty_death_does_not_prune_role_tabs() {
    assert_shutdown_death_preserves_role_tabs(ShutdownReason::Signal);
}

#[test]
fn window_exit_shutdown_pty_death_does_not_prune_role_tabs() {
    assert_shutdown_death_preserves_role_tabs(ShutdownReason::UserExit);
}

#[test]
fn restart_shutdown_pty_death_does_not_prune_role_tabs() {
    assert_shutdown_death_preserves_role_tabs(ShutdownReason::Restart);
}

#[test]
fn abrupt_pty_death_preserves_live_provider_sessions_for_restore() {
    for provider in ["claude", "codex", "opencode", "grok"] {
        let (fixture, tab_id, generation) = role_pty_fixture(AgentRole::Worker);
        let session_id = format!("{provider}-session");
        let nonce = format!("{provider}-nonce");
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).expect("worker tab");
            tab.agent_type = Some(provider.into());
            tab.agent_session_id = Some(session_id.clone());
            tab.session_binding_nonce = Some(nonce.clone());
            tab.reported_session_binding_nonce = Some(nonce);
        }

        let outcome =
            super::handle_pty_death_with_cause(&fixture.service, ROLE_PTY, generation, None);

        assert_eq!(
            outcome,
            super::PtyDeathOutcome::PreservedForRecovery,
            "{provider}"
        );
        let snapshot = fixture.service.mux.read().to_session_snapshot();
        let restored =
            crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);
        let restored_tab = restored.get_tab(&tab_id).expect("restored worker tab");
        assert_eq!(restored_tab.agent_type.as_deref(), Some(provider));
        assert_eq!(
            restored_tab.agent_session_id.as_deref(),
            Some(session_id.as_str())
        );
        assert_eq!(restored_tab.role, Some(AgentRole::Worker));
    }
}

#[test]
fn in_session_agent_death_without_shutdown_still_prunes_role_tabs() {
    for role in [AgentRole::Worker, AgentRole::Manager] {
        let (fixture, tab, generation) = role_pty_fixture(role);
        let parent = fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .and_then(|tab| tab.parent_tab_id.clone())
            .expect("role tab parent");
        let _parent =
            fixture.bind_recorded_agent_recipient(&parent, "parent-session", "parent-nonce");
        let outcome =
            super::handle_pty_death_with_cause(&fixture.service, ROLE_PTY, generation, None);
        assert_eq!(
            outcome,
            super::PtyDeathOutcome::Pruned,
            "{role:?} death with no shutdown armed must reach and complete the prune branch"
        );
        assert!(
            fixture.service.mux.read().get_tab(&tab).is_none(),
            "{role:?} tab must still be pruned when the agent dies in-session"
        );
    }
}
fn terminal_query_replies(data: &[u8], _headless: bool) -> Vec<&'static [u8]> {
    let mut replies = Vec::new();
    super::terminal_query_replies(
        data,
        &mut super::status_parser::StatusParser::default(),
        |reply| {
            replies.push(reply);
        },
    );
    replies
}

#[tokio::test]
async fn backend_reader_publishes_output_while_reply_writer_is_held() {
    use std::io::{Read, Write};
    use std::sync::mpsc;
    struct Reader(mpsc::Receiver<Vec<u8>>);
    impl Read for Reader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Ok(data) = self.0.recv() else {
                return Ok(0);
            };
            buf[..data.len()].copy_from_slice(&data);
            Ok(data.len())
        }
    }
    struct HeldWriter {
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        release: mpsc::Receiver<()>,
        wire: tokio::sync::mpsc::Sender<Vec<u8>>,
    }
    impl Write for HeldWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            self.wire.try_send(data.to_vec()).unwrap();
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (reader_tx, reader_rx) = mpsc::channel();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (wire_tx, mut wire_rx) = tokio::sync::mpsc::channel(4);
    let writer = codeterm_pty::pty::write_queue::WriteQueue::new(Box::new(HeldWriter {
        entered: Some(entered_tx),
        release: release_rx,
        wire: wire_tx,
    }));
    let replies =
        super::status_responder::ReplyOwner::with_writer("held-output", 19, move |reply| {
            writer.send_and_wait(reply.to_vec(), Duration::from_secs(5))
        });
    let (frontend_tx, mut frontend_rx) = tokio::sync::mpsc::channel(4);
    let reader_task = tokio::spawn(super::backend_read_loop(
        Box::new(Reader(reader_rx)),
        Arc::new(fixture.service.clone()),
        "held-output".into(),
        19,
        frontend_tx,
        replies,
        false,
    ));
    reader_tx.send(b"\x1b[5n".to_vec()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), frontend_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        b"\x1b[5n",
    );
    reader_tx.send(b"\x1b[5nOUTPUT-PROGRESS".to_vec()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), frontend_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        b"\x1b[5nOUTPUT-PROGRESS",
    );
    assert!(wire_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    for _ in 0..2 {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), wire_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            b"\x1b[0n",
        );
    }
    drop(reader_tx);
    tokio::time::timeout(Duration::from_secs(5), reader_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), wire_rx.recv())
            .await
            .unwrap(),
        None,
    );
}

#[tokio::test]
async fn backend_reader_from_current_thread_preserves_js_session_path_dispatch() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use codeterm_plugin::provider::AgentProvider;
    let manifest = PluginManifest::from_json_str(
        r#"{
        "id":"reader-runtime-fixture", "version":"1.0.0", "displayName":"Reader runtime fixture",
        "capabilities":{"agentProvider":true}
    }"#,
    )
    .unwrap();
    let vm = Arc::new(PluginVm::from_source("reader-runtime-fixture", r#"
        module.exports = {sessionFilePath: (_, session) => session === 'reader-owned-session' ? 'reader-owned.jsonl' : null};
    "#.into()).unwrap());
    let provider = JsProvider::new(manifest, vm);
    let (send, result) = tokio::sync::oneshot::channel();
    let reader = super::spawn_reader_task(async move {
        let path = tokio::task::spawn_blocking(move || {
            provider.session_file_path_checked("", "reader-owned-session")
        })
        .await
        .unwrap();
        send.send(path).unwrap();
    });
    reader.await.unwrap();
    assert_eq!(
        result.await.unwrap().unwrap(),
        Some(std::path::PathBuf::from("reader-owned.jsonl"))
    );
}
