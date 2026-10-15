use super::*;
use crate::multiplexer::types::Tab;

pub(crate) struct LiveRuntime {
    tab_id: String,
    _peer: tokio::io::DuplexStream,
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        stop(&self.tab_id);
    }
}

pub(crate) fn live_runtime(tab: &Tab) -> LiveRuntime {
    let (process, peer) = crate::async_runtime::block_on(async {
        let (host, peer) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(host);
        (
            Arc::new(Process::for_test(super::super::rpc::Client::attach(
                reader, writer,
            ))),
            peer,
        )
    });
    let mut state = RuntimeState::unloaded(tab.agent_session_id.clone());
    let epoch = state.begin();
    assert!(state.load(epoch, tab.agent_session_id.as_deref().unwrap()));
    let session = Arc::new(Session {
        provider: tab.agent_type.clone().unwrap(),
        adapter: Mutex::new(None),
        context: Mutex::new(RuntimeContext {
            epoch,
            tab_id: tab.id.clone(),
            binding_nonce: tab.session_binding_nonce.clone().unwrap(),
            thread_id: tab.agent_session_id.clone(),
            ..Default::default()
        }),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(Default::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        login: Mutex::new(Default::default()),
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(super::super::events::RuntimeMaps::default()),
        bookkeeping_failures: std::sync::atomic::AtomicU32::new(0),
        prompts: Mutex::new(HashMap::new()),
        process: Mutex::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
    });
    SESSIONS.lock().insert(tab.id.clone(), session);
    LiveRuntime {
        tab_id: tab.id.clone(),
        _peer: peer,
    }
}
