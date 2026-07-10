use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, SessionIdInjection, TaskDelivery,
};
use crate::agent::registry::AgentRegistry;
use crate::chatbackend::ChatBackendRegistry;
use crate::config::ConfigState;
use crate::cron::scheduler::{CronState, CronWake};
use crate::provider::ProviderRegistry;
use crate::service::{ServiceContext, ServiceState, WorkerPersonaReservations};
use crate::startup;
use crate::{KeepAwakeStateHandle, MeshStateHandle, ResizeOwnerState, UpdateStateHandle};

pub(crate) struct TestHarness {
    pub service: ServiceState,
    pub config: ConfigState,
    pub cron: CronState,
    pub cron_wake: CronWake,
    pub update: UpdateStateHandle,
    pub mesh: MeshStateHandle,
    pub keepawake: KeepAwakeStateHandle,
    pub resize_owner: ResizeOwnerState,
    pub events: crate::api::events::EventBroadcasterState,
}

pub(crate) struct TestProvider {
    id: &'static str,
}

impl TestProvider {
    pub(crate) fn new(id: &'static str) -> Self {
        Self { id }
    }
}

impl AgentProvider for TestProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.id
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        "echo ready".into()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn session_id_injection(&self) -> SessionIdInjection {
        SessionIdInjection::Fresh
    }
}

#[allow(invalid_value)]
pub(crate) fn test_harness(root: &Path, provider_id: &'static str) -> TestHarness {
    let states = startup::init_states();
    let config = states.config.clone();
    let update = states.update.clone();
    let mesh = states.mesh.clone();
    let keepawake = states.keepawake.clone();
    let resize_owner = states.resize_owner.clone();
    let events = states.event_broadcaster.clone();
    let cron: CronState = Arc::new(Mutex::new(crate::cron::store::CronStore::new()));
    let cron_wake: CronWake = Arc::new(std::sync::Condvar::new());

    let agent_registry = Arc::new(AgentRegistry::new());
    agent_registry.register_provider(Box::new(TestProvider::new(provider_id)));
    let chatbackend_registry = Arc::new(ChatBackendRegistry::new());
    let provider_registry = Arc::new(ProviderRegistry::new(
        agent_registry.clone(),
        chatbackend_registry.clone(),
    ));
    let secret_registry = Arc::new(crate::secrets::SecretRegistry::new());
    let temp_root = root.join("service-state");
    std::fs::create_dir_all(&temp_root).expect("temp root");
    let plan_store = Arc::new(crate::service::plan::PlanStore::new_for_test(root));
    let plan_turn_counter = Arc::new(crate::service::plan::reminders::TurnCounter::new());
    plan_store.bind_turn_counter(plan_turn_counter.clone());

    let service: ServiceState = Arc::new(ServiceContext {
        mux: states.mux.clone(),
        pty: states.pty.clone(),
        output_buf: states.output_buffer.clone(),
        screen_state: states.screen_state.clone(),
        notif: states.notif.clone(),
        events: events.clone(),
        app_handle: unsafe { std::mem::MaybeUninit::zeroed().assume_init() },
        agent_tracker: states.agent_tracker.clone(),
        shell_caps: Arc::new(parking_lot::Mutex::new(Default::default())),
        report_store: states.report_store.clone(),
        agent_registry: agent_registry.clone(),
        mem: crate::mem::MemManager::open(&temp_root).expect("mem"),
        mem_supervisor: Arc::new(crate::mem::supervisor::Supervisor::new(
            crate::mem::supervisor::Supervisor::resolve_cli_path(&temp_root),
            temp_root.join("mem.db"),
        )),
        activity: crate::mem::activity::ActivityRing::new(64),
        workers: crate::mem::workers::GraphWorkerRegistry::load(temp_root.join("workers.json")),
        plugin_activity: Arc::new(crate::plugin::activity::PluginActivityRegistry::new()),
        worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(1)),
        prompt_detector: Arc::new(Mutex::new(crate::agent::prompt::PromptDetector::new())),
        agent_run_pty_map: Arc::new(parking_lot::Mutex::new(Default::default())),
        run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
        remote_workers: Arc::new(crate::service::remote_workers::RemoteWorkerRegistry::load(
            temp_root.join("remote_workers.json"),
        )),
        bridge_outbound: crate::bridge::manager::create_outbound_channel().0,
        bridge_outbound_rx: Arc::new(Mutex::new(None)),
        bridge_supervisor: Arc::new(crate::bridge::supervisor::BridgeSupervisor::new()),
        restart_tracker: crate::service::agent_restart::new_restart_tracker(),
        chat_registry: crate::service::pane_chat::ChatRegistry::new(),
        delayed_send: crate::delayed_send::DelayedSendQueue::open(
            temp_root.join("delayed_send.json"),
        )
        .expect("delayed send"),
        secrets: crate::secrets::new_state(
            &config.lock().integrations.secrets.clone(),
            &secret_registry,
        ),
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
        mux_emit_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        session_persist_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        services_store: Arc::new(
            crate::service::store::ServicesStore::open(&temp_root.join("services.db"))
                .expect("services"),
        ),
        tokens: Arc::new(crate::service::tokens::TokenStore::new()),
        pasteboard: crate::pasteboard::PasteboardState::new(),
        input_parser: Arc::new(crate::service::input_parser::InputParser::new(
            agent_registry,
        )),
        usage_store: crate::usage::UsageStore::open(&temp_root).expect("usage"),
        usage_activity: crate::usage::PaneActivity::new(),
        user_last_activity: crate::user_last_activity::UserLastActivity::new(),
        usage_poller: Arc::new(parking_lot::Mutex::new(None)),
        plan_store,
        plan_watchers: Arc::new(parking_lot::Mutex::new(Default::default())),
        loaded_plan_projects: Arc::new(parking_lot::Mutex::new(Default::default())),
        plan_startup_loaded: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        plan_turn_counter,
        worker_identity_allocator: Arc::new(parking_lot::Mutex::new(
            WorkerPersonaReservations::default(),
        )),
    });
    TestHarness {
        service,
        config,
        cron,
        cron_wake,
        update,
        mesh,
        keepawake,
        resize_owner,
        events,
    }
}
