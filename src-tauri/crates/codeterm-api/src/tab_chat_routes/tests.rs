use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::Arc;

fn shared_mux() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    (Arc::new(TimedRwLock::new("mux-test", state)), tab_id)
}

#[test]
fn transcript_contention_leaves_the_runtime_available() {
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use std::{future::Future, sync::mpsc, task::Poll, time::Duration};

    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let mux = state.svc.mux.clone();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let guard = mux.write();
        held_tx.send(()).unwrap();
        let released = release_rx.recv_timeout(Duration::from_secs(2)).is_ok();
        drop(guard);
        released
    });
    held_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime.block_on(async {
        let mut request = Box::pin(snapshot(
            AxumState(state),
            Path(tab_id),
            Query(ChatQuery { since: 0 }),
            HeaderMap::new(),
        ));
        std::future::poll_fn(|cx| {
            assert!(request.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        release_tx.send(()).unwrap();
        request.await.unwrap()
    });
    assert!(holder.join().unwrap());
    assert_eq!(
        response.0,
        serde_json::json!({ "messages": [], "count": 0, "pending": true })
    );
}

#[test]
fn unknown_tab_is_not_found_rather_than_another_tabs_transcript() {
    let (mux, _tab_id) = shared_mux();
    let err = chat_read_target(&mux, "no-such-tab").unwrap_err();
    assert_eq!(err.0, StatusCode::NOT_FOUND);
}

#[test]
fn unbound_tab_reads_empty_even_when_a_sibling_owns_a_session() {
    let (mux, tab_id) = shared_mux();
    let sibling = mux.write().create_tab_record();
    {
        let mut m = mux.write();
        let owner = m.get_tab_mut(&sibling).unwrap();
        owner.agent_type = Some("provider-a".into());
        owner.agent_session_id = Some("sid-owned-by-sibling".into());
    }
    assert_eq!(
        chat_read_target(&mux, &tab_id).unwrap(),
        ChatReadTarget::Unbound
    );
}

#[test]
fn agent_tab_without_a_bound_session_reads_empty_not_pending_forever() {
    let (mux, tab_id) = shared_mux();
    {
        let mut m = mux.write();
        let tab = m.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("provider-a".into());
        tab.agent_session_id = None;
    }
    assert_eq!(
        chat_read_target(&mux, &tab_id).unwrap(),
        ChatReadTarget::Unbound
    );
}

#[test]
fn tab_owning_a_session_resolves_that_session() {
    let (mux, tab_id) = shared_mux();
    {
        let mut m = mux.write();
        let tab = m.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("provider-a".into());
        tab.agent_session_id = Some("sid-owned".into());
    }
    assert_eq!(
        chat_read_target(&mux, &tab_id).unwrap(),
        ChatReadTarget::OwnedSession
    );
}

#[test]
fn chat_backend_tab_resolves_its_own_backing_session() {
    let (mux, tab_id) = shared_mux();
    {
        let mut m = mux.write();
        let tab = m.get_tab_mut(&tab_id).unwrap();
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "plugin-a".into(),
            session_id: "sid-backing".into(),
        };
    }
    assert_eq!(
        chat_read_target(&mux, &tab_id).unwrap(),
        ChatReadTarget::OwnedSession
    );
}
