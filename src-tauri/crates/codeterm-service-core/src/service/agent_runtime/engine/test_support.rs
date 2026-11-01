pub(in crate::service::agent_runtime) use super::tests::sync_send::runtime_fixture;
use super::*;
use crate::multiplexer::types::Tab;

pub(crate) struct LiveRuntime {
    tab_id: String,
    pub(crate) peer: tokio::io::DuplexStream,
}

impl Drop for LiveRuntime {
    fn drop(&mut self) {
        stop_test(&self.tab_id);
    }
}

pub(crate) fn live_runtime(tab: &Tab) -> LiveRuntime {
    loaded_runtime(tab, None)
}

pub(crate) fn loaded_runtime(
    tab: &Tab,
    adapter: Option<Arc<dyn crate::agent::provider::AgentProvider>>,
) -> LiveRuntime {
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
        update_generation: std::sync::atomic::AtomicU64::new(0),
        update_recovery_available: std::sync::atomic::AtomicBool::new(false),
        provider: tab.agent_type.clone().unwrap(),
        adapter: Mutex::new(adapter),
        context: Mutex::new(RuntimeContext {
            restricted_cli: None,
            host_owned: Default::default(),
            epoch,
            tab_id: tab.id.clone(),
            binding_nonce: tab.session_binding_nonce.clone().unwrap(),
            thread_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone().unwrap_or_default(),
            host_version: codeterm_build_info::version().into(),
            ..Default::default()
        }),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
        reverted_thread: Mutex::new(None),
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(Default::default()),
        auth: Mutex::new(RuntimeAuthState::SignedIn),
        login: Mutex::new(Default::default()),
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        hook_state: Mutex::new(Value::Null),
        maps: Mutex::new(super::super::events::RuntimeMaps::default()),
        provider_operation_failures: std::sync::atomic::AtomicU32::new(0),
        bookkeeping_warning_emitted: std::sync::atomic::AtomicBool::new(false),
        prompts: Mutex::new(HashMap::new()),
        process: crate::service::agent_runtime::process_slot::ProcessSlot::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
        history_retry: Mutex::new(None),
        capacity_retry: Mutex::new(Default::default()),
    });
    SESSIONS.lock().insert(tab.id.clone(), session);
    LiveRuntime {
        tab_id: tab.id.clone(),
        peer,
    }
}

pub(super) fn stop_test(tab: &str) {
    let session = SESSIONS.lock().remove(tab);
    if let Some(session) = session {
        if let Some(process) = session.process.close() {
            crate::async_runtime::block_on(process.finish()).unwrap();
        }
    }
}
