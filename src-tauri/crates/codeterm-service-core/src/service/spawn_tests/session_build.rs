use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn hidden_agent_session_build_releases_pty_and_mux_before_register_and_link() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let gate = service.pty.lock().spawn_gate();
    let built = crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
        .expect("prepare backing session outside the measured build");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_service = service.clone();
    let worker_tab = tab_id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.link_hidden_agent_pty_with_builder(&worker_tab, move |_| {
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release fake builder");
            Ok(built)
        })
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("builder entered");
    let pty_available = service.pty.try_lock().is_some();
    let mux_available = service.mux.try_read().is_some();
    release_tx.send(()).unwrap();
    let pty_id = worker.join().unwrap().expect("register and link session");
    assert!(pty_available, "builder must not own the PTY manager");
    assert!(mux_available, "builder must not own the mux");
    assert!(service.pty.lock().get_session(&pty_id).is_some());
    assert_eq!(
        service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pty_id
            .as_deref(),
        Some(pty_id.as_str())
    );
    assert_eq!(service.agent_run_pty_map.lock().get(&tab_id), Some(&pty_id));
    service.kill_ptys(&[pty_id]);
}
