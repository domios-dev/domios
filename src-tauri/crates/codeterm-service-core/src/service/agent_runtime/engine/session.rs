use super::*;

pub struct OutstandingRequest {
    pub request: PendingRuntimeRequest,
    pub prompt: Option<codeterm_types::pty::PendingPrompt>,
    pub permission: Option<codeterm_plugin::agent_runtime::RuntimePermission>,
}

pub struct Session {
    pub(in crate::service::agent_runtime) capacity_retry:
        Mutex<super::super::capacity_retry::RetryState>,
    pub(in crate::service::agent_runtime) update_generation: std::sync::atomic::AtomicU64,
    pub(in crate::service::agent_runtime) update_recovery_available: std::sync::atomic::AtomicBool,
    pub provider: String,
    pub(in crate::service::agent_runtime) account: Mutex<String>,
    pub(in crate::service::agent_runtime) invocation:
        Mutex<Option<Arc<codeterm_plugin::provider_account::InvocationContext>>>,
    pub adapter: Mutex<Option<Arc<dyn crate::agent::provider::AgentProvider>>>,
    pub context: Mutex<RuntimeContext>,
    pub(in crate::service::agent_runtime) controls_confirmation: Mutex<Option<RuntimeSettings>>,
    pub control_changes: tokio::sync::watch::Sender<()>,
    pub(in crate::service::agent_runtime) reverted_thread: Mutex<Option<String>>,
    pub(in crate::service::agent_runtime) completions:
        Mutex<super::super::completions::Completions>,
    pub state: Mutex<RuntimeState>,
    pub cache: Mutex<ItemCache>,
    pub auth: Mutex<RuntimeAuthState>,
    pub(in crate::service::agent_runtime) credential_stamp:
        Mutex<Option<super::credential_stamp::CredentialStamp>>,
    pub(in crate::service::agent_runtime) login: Mutex<super::super::login::LoginState>,
    pub thread_loaded: std::sync::atomic::AtomicBool,
    pub hook_state: Mutex<Value>,
    pub(in crate::service::agent_runtime) stream_progress: tokio::sync::watch::Sender<u64>,
    pub(in crate::service::agent_runtime) turn_requests:
        Mutex<HashMap<String, Arc<super::super::turn_requests::PendingTurn>>>,
    pub maps: Mutex<super::super::events::RuntimeMaps>,
    pub(in crate::service::agent_runtime) provider_operation_failures: std::sync::atomic::AtomicU32,
    pub(in crate::service::agent_runtime) bookkeeping_warning_emitted:
        std::sync::atomic::AtomicBool,
    pub(in crate::service::agent_runtime) transcript_io: super::super::transcript::TranscriptIo,
    pub prompts: Mutex<HashMap<RpcId, OutstandingRequest>>,
    pub(in crate::service::agent_runtime) prompts_cancelled: std::sync::atomic::AtomicBool,
    pub process: ProcessSlot<Arc<Process>>,
    pub gate: tokio::sync::Semaphore,
    pub(in crate::service::agent_runtime) initialization: super::super::initialize::Initialization,
    pub(in crate::service::agent_runtime) compaction:
        Mutex<super::super::subscriber::CompactionState>,
    pub(in crate::service::agent_runtime) abort_note: Mutex<Option<String>>,
    pub(in crate::service::agent_runtime) history_retry: Mutex<Option<backfill::FailedReplay>>,
}
