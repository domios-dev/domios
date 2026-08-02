//! Shared test-only ServiceContext harness backed by a mock Tauri app.
//! Requires a desktop runtime; tests using it are `ignore`d off Windows.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::Mutex;

use super::ServiceContext;
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::PlanStore;

pub(crate) struct ServiceFixture {
    pub(crate) service: ServiceContext,
    pub(crate) _app: tauri::App,
    pub(crate) temp: tempfile::TempDir,
}

impl ServiceFixture {
    pub(crate) fn new(mux: MultiplexerState) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let builder = tauri::Builder::default();
        #[cfg(windows)]
        let builder = builder.any_thread();
        let app = builder
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let agent_registry = Arc::new(crate::agent::registry::AgentRegistry::new());
        let chatbackend_registry = Arc::new(crate::chatbackend::ChatBackendRegistry::new());
        let provider_registry = Arc::new(crate::provider::ProviderRegistry::new(
            agent_registry.clone(),
            chatbackend_registry.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new_for_test(temp.path()));
        let plan_turn_counter = Arc::new(crate::service::plan::reminders::TurnCounter::new());
        plan_store.bind_turn_counter(plan_turn_counter.clone());
        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let mem_supervisor = Arc::new(crate::mem::supervisor::Supervisor::new(
            temp.path().join("codeterm"),
            mem.db_path().to_path_buf(),
        ));
        let secret_registry = Arc::new(crate::secrets::SecretRegistry::new());
        let secrets = crate::secrets::new_state(
            &crate::config::schema::SecretsConfig::default(),
            &secret_registry,
        );
        let (bridge_outbound, bridge_outbound_rx) =
            crate::bridge::manager::create_outbound_channel();
        let pending_delivery_payloads = Arc::new(
            crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
                temp.path(),
                &HashSet::new(),
            )
            .unwrap(),
        );
        let services_store = Arc::new(
            crate::service::store::ServicesStore::open(&temp.path().join("services.db")).unwrap(),
        );
        let usage_store = crate::usage::UsageStore::open(temp.path()).unwrap();
        let service = ServiceContext {
            mux: Arc::new(TimedRwLock::new("mux", mux)),
            pty: Arc::new(TimedMutex::new(
                "pty",
                crate::pty::manager::PtyManager::new(),
            )),
            output_buf: Arc::new(TimedMutex::new(
                "output_buf",
                crate::pty::manager::OutputBuffer::new(65_536),
            )),
            screen_state: Arc::new(TimedMutex::new(
                "screen_state",
                crate::pty::screen_state::ScreenState::new(),
            )),
            notif: Arc::new(Mutex::new(
                crate::notification::store::NotificationStore::new(10),
            )),
            events: Arc::new(crate::api::events::EventBroadcaster::new()),
            app_handle: app.handle().clone(),
            agent_tracker: Arc::new(TimedMutex::new(
                "agent_tracker",
                crate::agent::status::TabStatusTracker::new(),
            )),
            shell_caps: Arc::new(Mutex::new(Default::default())),
            report_store: Arc::new(Mutex::new(crate::agent::reports::ReportStore::new(10))),
            kpi: crate::agent::kpi::KpiRecorder::disabled(),
            agent_registry: agent_registry.clone(),
            mem,
            mem_supervisor,
            activity: crate::mem::activity::ActivityRing::new(10),
            workers: crate::mem::workers::GraphWorkerRegistry::load(
                temp.path().join("workers.json"),
            ),
            plugin_activity: Arc::new(crate::plugin::activity::PluginActivityRegistry::new()),
            worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(1)),
            prompt_detector: Arc::new(Mutex::new(crate::agent::prompt::PromptDetector::new())),
            pty_ownership: Default::default(),
            agent_run_pty_map: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
            remote_workers: Arc::new(crate::service::remote_workers::RemoteWorkerRegistry::new()),
            bridge_outbound,
            bridge_outbound_rx: Arc::new(Mutex::new(Some(bridge_outbound_rx))),
            bridge_supervisor: Arc::new(crate::bridge::supervisor::BridgeSupervisor::new()),
            restart_tracker: crate::service::agent_restart::new_restart_tracker(),
            provider_updates: Default::default(),
            chat_registry: crate::service::tab_chat::ChatRegistry::new(),
            delayed_send: crate::delayed_send::DelayedSendQueue::open(
                temp.path().join("delayed-send.json"),
            )
            .unwrap(),
            secrets,
            secret_registry,
            transcribe_registry: Arc::new(crate::transcribe::TranscribeRegistry::new()),
            glance_registry: Arc::new(crate::glance::GlanceRegistry::new()),
            bubble_registry: Arc::new(crate::bubble::BubbleRegistry::new()),
            view_registry: Arc::new(crate::view_host::ViewRegistry::new()),
            chatbackend_registry,
            chatbackend_reopen_guards: Arc::new(Default::default()),
            provider_registry,
            chat_preprocessor_registry: Arc::new(
                crate::chatpreprocessor::ChatPreprocessorRegistry::new(),
            ),
            agent_command_registry: Arc::new(crate::agent_commands::AgentCommandRegistry::new()),
            mux_emit_pending: Arc::new(AtomicBool::new(false)),
            mux_emit_stats: Arc::new(crate::service::MuxEmitStats::default()),
            session_persist_pending: Arc::new(AtomicBool::new(false)),
            services_store,
            tokens: Arc::new(crate::service::tokens::TokenStore::new_with_root(
                temp.path().join("tokens"),
            )),
            pasteboard: crate::pasteboard::PasteboardState::new(),
            input_parser: Arc::new(crate::service::input_parser::InputParser::new(
                agent_registry,
            )),
            usage_store,
            usage_activity: crate::usage::TabActivity::new(),
            user_last_activity: crate::user_last_activity::UserLastActivity::new(),
            usage_poller: Arc::new(Mutex::new(None)),
            plan_store,
            plan_watchers: Arc::new(Mutex::new(HashMap::new())),
            loaded_plan_projects: Arc::new(Mutex::new(HashSet::new())),
            plan_startup_loaded: Arc::new(AtomicBool::new(false)),
            plan_turn_counter,
            pending_delivery_payloads,
            chat_send_ledger: Arc::new(Default::default()),
            worker_identity_allocator: Arc::new(Mutex::new(Default::default())),
            worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
            gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
        };
        Self {
            service,
            _app: app,
            temp,
        }
    }
}
