use super::*;
use crate::pty::write_queue::WriteQueue;

fn strict() -> ShutdownPolicy<'static> {
    ShutdownPolicy {
        require_idle: true,
        ..Default::default()
    }
}

fn snapshot(queue: &WriteQueue) -> HolderTab {
    HolderTab {
        process: HolderProcess {
            pid: 42,
            created: 123,
        },
        tab_id: "tab-build".into(),
        title: "Build".into(),
        workspace: "Project".into(),
        command: "idle shell".into(),
        busy: false,
        pty_id: "pty-build".into(),
        pty_generation: 1,
        input_revision: queue.input_revision(),
    }
}

#[test]
fn idle_to_busy_before_shutdown_refuses_and_releases_every_input_reservation() {
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    let expected = snapshot(&queue);
    let mut current = expected.clone();
    current.busy = true;
    current.command = "node long-task.js".into();
    let state = Arc::new(Mutex::new(GateState::default()));
    let result = reserve_with(
        &state,
        &[expected],
        strict(),
        |tab| queue.reserve_update_shutdown(Some(tab.input_revision)),
        || Ok(vec![current]),
    );
    let refusal = result.as_ref().unwrap_err();
    assert!(refusal.contains("Build in Project: node long-task.js"));
    assert!(state.lock().reservation.is_none());
    assert!(queue.admit_work().is_ok());
    assert!(queue
        .send_and_wait(
            b"original shell survives".to_vec(),
            std::time::Duration::from_secs(1)
        )
        .is_ok());
}

#[test]
fn new_input_after_preflight_refuses_even_before_busy_state_is_reported() {
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    let expected = snapshot(&queue);
    queue.admit_work().unwrap();
    let state = Arc::new(Mutex::new(GateState::default()));
    let result = reserve_with(
        &state,
        &[expected.clone()],
        strict(),
        |tab| queue.reserve_update_shutdown(Some(tab.input_revision)),
        || Ok(vec![expected]),
    );
    assert!(result.unwrap_err().contains("Build in Project"));
    assert!(state.lock().reservation.is_none());
}

#[test]
fn committed_reservation_blocks_new_admission_and_raw_writes_until_release() {
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    let expected = snapshot(&queue);
    let state = Arc::new(Mutex::new(GateState::default()));
    reserve_with(
        &state,
        &[expected.clone()],
        strict(),
        |tab| queue.reserve_update_shutdown(Some(tab.input_revision)),
        || Ok(vec![expected]),
    )
    .unwrap();
    assert!(queue.admit_work().is_err());
    assert!(crate::pty::write_queue::is_update_reserved(
        &queue.send(b"new task".to_vec()).unwrap_err()
    ));
    state.lock().reservation = None;
    assert!(queue.admit_work().is_ok());
}

#[test]
fn active_service_admission_prevents_shutdown_without_freezing_or_teardown() {
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    let expected = snapshot(&queue);
    let state = Arc::new(Mutex::new(GateState::default()));
    state.lock().active.insert(expected.tab_id.clone(), 1);
    let result = reserve_with(
        &state,
        &[expected],
        strict(),
        |_| panic!("in-flight work must refuse before freezing"),
        || panic!("no shutdown authority"),
    );
    assert!(result.unwrap_err().contains("input admission is in flight"));
    assert!(state.lock().reservation.is_none());
}

#[test]
fn server_reservation_blocks_agent_and_shell_admission_and_requires_its_release_id() {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    let mut expected = snapshot(&queue);
    expected.tab_id = tab.clone();
    let gate = svc.ext_or_init(Gate::default).0.clone();
    let id = reserve_with(
        &gate,
        &[expected.clone()],
        strict(),
        |holder| queue.reserve_update_shutdown(Some(holder.input_revision)),
        || {
            assert!(svc.admit_update_input(&tab).is_err());
            Ok(vec![expected])
        },
    )
    .unwrap();
    assert!(svc.admit_update_input(&tab).is_err());
    assert!(svc
        .send_shell_spawn_observed(&tab, "must not run")
        .err()
        .unwrap()
        .contains("update shutdown"));
    let agent = crate::service::messenger::Messenger::deliver(
        svc,
        &tab,
        Default::default(),
        "must not reach an agent",
    );
    assert!(agent.unwrap_err().contains("update shutdown"));
    assert!(svc.release_update_shutdown("another-operation").is_err());
    assert!(svc.admit_update_input(&tab).is_err());
    svc.release_update_shutdown(&id).unwrap();
    assert!(svc.admit_update_input(&tab).is_ok());
}

#[test]
fn pty_retirement_fences_input_only_while_it_owns_the_tab() {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let claim = svc.pty_ownership.acquire(&tab).unwrap();

    let input = svc.admit_update_input(&tab).unwrap();
    assert!(svc.reserve_pty_input_retirement(&claim).is_err());
    drop(input);

    let retirement = svc.reserve_pty_input_retirement(&claim).unwrap();
    assert!(svc.admit_update_input(&tab).is_err());
    assert!(svc.admit_update_input("another-tab").is_ok());
    drop(retirement);
    assert!(svc.admit_update_input(&tab).is_ok());
}

#[test]
fn changed_holder_identity_cannot_authorize_shutdown() {
    for change in ["process", "pty", "generation"] {
        let queue = WriteQueue::new(Box::new(std::io::sink()));
        let expected = snapshot(&queue);
        let mut current = expected.clone();
        match change {
            "process" => current.process.created += 1,
            "pty" => current.pty_id = "replacement-pty".into(),
            _ => current.pty_generation += 1,
        }
        let state = Arc::new(Mutex::new(GateState::default()));
        let refused = reserve_with(
            &state,
            &[expected],
            strict(),
            |tab| queue.reserve_update_shutdown(Some(tab.input_revision)),
            || Ok(vec![current]),
        );
        assert!(refused
            .unwrap_err()
            .contains("holder identity changed during reservation"));
        assert!(state.lock().reservation.is_none());
        assert!(queue.admit_work().is_ok());
    }
}

#[test]
fn active_holder_reserves_by_default_and_the_initiator_reserves_in_idle_only_mode() {
    for require_idle in [false, true] {
        let queue = WriteQueue::new(Box::new(std::io::sink()));
        let mut busy = snapshot(&queue);
        busy.busy = true;
        queue.admit_work().unwrap();
        let gate = Arc::new(Mutex::new(GateState::default()));
        gate.lock().active.insert(busy.tab_id.clone(), 1);
        let policy = ShutdownPolicy {
            require_idle,
            initiating_tab: require_idle.then_some(busy.tab_id.as_str()),
        };
        reserve_with(
            &gate,
            std::slice::from_ref(&busy),
            policy,
            |tab| {
                queue.reserve_update_shutdown(
                    policy
                        .requires_idle(&tab.tab_id)
                        .then_some(tab.input_revision),
                )
            },
            || Ok(vec![busy.clone()]),
        )
        .unwrap();
        assert!(queue.admit_work().is_err());
        gate.lock().reservation = None;
        assert!(queue.admit_work().is_ok());
    }
}
