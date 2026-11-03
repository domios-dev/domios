use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn delayed_cleanup_does_not_remove_a_replacement_generation() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let pty = "replacement-memory-fixture";
    assert!(svc.screen_state.bind_pty_generation(pty, 9));
    assert!(svc.prepare_pty_output(pty, Some(9), b"replacement"));
    svc.retire_pty_memory(&[(pty.into(), Some(8))]);
    assert_eq!(svc.screen_state.pty_generation(pty), Some(9));
    assert_eq!(
        svc.output_buf.lock().copy_full(pty).unwrap(),
        b"replacement"
    );
}

#[test]
fn killed_pty_releases_buffers_and_late_frames_cannot_recreate_them() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.link_pty(&tab, "closed-memory-fixture".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let pty = "closed-memory-fixture";
    assert!(svc.screen_state.bind_pty_generation(pty, 7));
    assert!(svc.prepare_pty_output(pty, Some(7), b"retained output"));
    assert!(svc.output_buf.lock().full_len(pty) > 0);
    svc.close_tab(&tab).unwrap();
    assert!(!svc.screen_state.contains_pty(pty));
    assert_eq!(svc.output_buf.lock().memory_stats()["count"], 0);
    assert!(!svc.prepare_pty_output(pty, Some(7), b"late output"));
    assert_eq!(svc.output_buf.lock().memory_stats()["count"], 0);
    assert!(!svc.screen_state.contains_pty(pty));
}

#[test]
fn closing_one_tab_preserves_live_output_for_another_tab() {
    let (mut mux, closed_tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&closed_tab).unwrap();
    let open_tab = mux.create_tab(&workspace).unwrap();
    mux.link_pty(&closed_tab, "closed-pty".into());
    mux.link_pty(&open_tab, "open-pty".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    assert!(svc.screen_state.bind_pty_generation("closed-pty", 7));
    assert!(svc.screen_state.bind_pty_generation("open-pty", 8));
    assert!(svc.prepare_pty_output("closed-pty", Some(7), b"closed"));
    assert!(svc.prepare_pty_output("open-pty", Some(8), b"open"));
    svc.close_tab(&closed_tab).unwrap();
    assert!(svc.mux.read().get_tab(&open_tab).is_some());
    assert!(svc.prepare_pty_output("open-pty", Some(8), b" appended"));
    assert_eq!(
        svc.output_buf.lock().copy_full("open-pty").unwrap(),
        b"open appended"
    );
    assert!(svc.screen_state.contains_pty("open-pty"));
}

#[test]
fn retained_run_logs_keep_bytes_but_release_screen_and_reject_late_frames() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let pty = "retained-run-memory-fixture";
    svc.agent_run_pty_map
        .lock()
        .insert("run".into(), pty.into());
    assert!(svc.screen_state.bind_pty_generation(pty, 8));
    assert!(svc.prepare_pty_output(pty, Some(8), b"completed run"));
    svc.kill_ptys(&[pty.into()]);
    assert!(!svc.screen_state.contains_pty(pty));
    assert!(!svc.prepare_pty_output(pty, Some(8), b"late output"));
    assert_eq!(
        svc.output_buf.lock().copy_full(pty).unwrap(),
        b"completed run"
    );
}
