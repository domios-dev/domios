use crate::multiplexer::state::MultiplexerState;
use crate::service::pty_ownership::OwnershipEvent;
use crate::service::test_fixture::ServiceFixture;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn blocked_shell_builder_releases_global_locks_and_registers_once() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let gate = service.pty.lock().spawn_gate();
    let built = crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
        .expect("prepare session before fake builder");
    let session = built.session.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    service
        .pty_ownership
        .set_test_observer(Arc::new(move |_, event| {
            if event == OwnershipEvent::AwaitingActivation {
                waiting_tx.send(()).unwrap();
            }
        }));
    let worker_service = service.clone();
    let worker_tab = tab_id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.link_hidden_shell_pty_with_builder(&worker_tab, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).expect("release builder");
            Ok(built)
        })
    });
    started_rx.recv_timeout(DEADLINE).expect("builder entered");
    {
        let pty = service.pty.try_lock().expect("another tab can use PTYs");
        assert!(pty.get_session("another-tab").is_none());
    }
    {
        let mux = service.mux.try_read().expect("builder releases mux");
        assert!(mux.get_tab(&tab_id).unwrap().pty_id.is_none());
    }
    let second_service = service.clone();
    let second_tab = tab_id.clone();
    let second = std::thread::spawn(move || {
        second_service.link_hidden_shell_pty_with_builder(&second_tab, |_| {
            panic!("a second activation must reuse the registered session")
        })
    });
    waiting_rx
        .recv_timeout(DEADLINE)
        .expect("second activation waits");
    release_tx.send(()).unwrap();
    let pty_id = worker.join().unwrap().expect("register session");
    assert_eq!(second.join().unwrap().unwrap(), pty_id);
    let registered = service.pty.lock().get_session(&pty_id).unwrap();
    assert!(Arc::ptr_eq(&registered, &session));
    assert_eq!(registered.generation, session.generation);
    assert_eq!(service.resolve_pty_id(&tab_id).unwrap(), pty_id);
    service.kill_ptys(&[pty_id]);
}

#[test]
fn shell_closed_during_build_reaps_the_unlinked_session() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let gate = service.pty.lock().spawn_gate();
    let built = crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
        .expect("prepare session before fake builder");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (candidate_tx, candidate_rx) = mpsc::channel();
    service
        .pty_ownership
        .set_test_observer(Arc::new(move |_, event| {
            if let OwnershipEvent::Linking { candidate, .. } = event {
                candidate_tx.send(candidate).unwrap();
            }
        }));
    let worker_service = service.clone();
    let worker_tab = tab_id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.link_hidden_shell_pty_with_builder(&worker_tab, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).expect("release builder");
            Ok(built)
        })
    });
    started_rx.recv_timeout(DEADLINE).expect("builder entered");
    assert!(service.mux.write().close_tab(&tab_id).unwrap().is_empty());
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap_err().contains("closed"));
    let candidate = candidate_rx
        .recv_timeout(DEADLINE)
        .expect("candidate linked once");
    assert!(candidate_rx.try_recv().is_err());
    assert!(service.pty.lock().get_session(&candidate).is_none());
    assert!(service.mux.read().get_tab(&tab_id).is_none());
}

#[test]
fn failed_shell_build_leaves_no_pty_link() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let error = fixture
        .service
        .link_hidden_shell_pty_with_builder(&tab_id, |_| Err("spawn failed".into()))
        .unwrap_err();
    assert_eq!(error, "spawn failed");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pty_id
        .is_none());
}
