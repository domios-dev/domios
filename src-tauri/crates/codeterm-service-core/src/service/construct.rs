use super::ServiceContext;
use crate::bridge;
#[cfg(any(test, feature = "test-support"))]
use crate::service;
use parking_lot::Mutex;
use std::sync::Arc;

pub fn build_service_context(
    states: &crate::startup::AppStates,
    app_handle: &crate::app_runtime::AppHandle,
    agent_registry: Arc<crate::agent::registry::AgentRegistry>,
    chatbackend_registry: &crate::chatbackend::ChatBackendRegistryHandle,
    mem: Arc<crate::mem::MemManager>,
    prompt_detector: crate::PromptDetectorState,
    bridge_outbound_tx: crate::bridge::manager::OutboundSender,
    bridge_outbound_rx: tokio::sync::mpsc::UnboundedReceiver<
        crate::bridge::manager::OutboundMessage,
    >,
    secret_registry: &Arc<crate::secrets::SecretRegistry>,
    transcribe_registry: &Arc<crate::transcribe::TranscribeRegistry>,
    converter_registry: &Arc<codeterm_plugin::converter::ConverterRegistry>,
    graph_node_registry: &Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>,
    event_source_registry: &Arc<codeterm_plugin::event_source::EventSourceRegistry>,
    graph_event_subscriptions: &codeterm_plugin::graph_events::SubscriptionTable,
    decision_model_registry: Arc<crate::service::supporter::registry::SupporterRegistry>,
    glance_registry: &crate::glance::GlanceRegistryHandle,
    bubble_registry: &crate::bubble::BubbleRegistryHandle,
    repo_watch: crate::repowatch::RepoWatchRegistryHandle,
    view_registry: &crate::view_host::ViewRegistryHandle,
    chat_preprocessor_registry: &crate::chatpreprocessor::ChatPreprocessorRegistryHandle,
    agent_command_registry: &crate::agent_commands::AgentCommandRegistryHandle,
    webhook_registry: &codeterm_plugin::WebhookRegistryHandle,
    tab_menu_registry: &codeterm_plugin::tab_menu::TabMenuRegistryHandle,
    surface_registry: codeterm_plugin::surfaces::SurfaceRegistryHandle,
    cap_registries: &crate::plugin::js_runtime::CapabilityRegistries,
) -> ServiceContext {
    let mem_supervisor = Arc::new(crate::mem::supervisor::Supervisor::new(
        crate::mem::supervisor::Supervisor::resolve_cli_path(&crate::paths::data_dir()),
        mem.db_path().to_path_buf(),
    ));

    let input_parser = Arc::new(crate::service::input_parser::InputParser::new(
        agent_registry.clone(),
    ));
    let provider_registry = Arc::new(crate::provider::ProviderRegistry::new(
        agent_registry.clone(),
        chatbackend_registry.clone(),
    ));
    crate::service::plan::search_index::init_search_port(Arc::new(
        crate::service::plan::search_port_impl::MemSearchPort,
    ));
    let plan_store = Arc::new(crate::service::plan::PlanStore::new());
    let plan_turn_counter = Arc::new(crate::service::plan::reminders::TurnCounter::new());
    plan_store.bind_turn_counter(plan_turn_counter.clone());
    let live_pending_delivery_digests =
        crate::service::pending_delivery_payloads::live_payload_digests(
            &states.mux.read().snapshot(),
        );
    let pending_delivery_payloads = Arc::new(
        crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
            &crate::paths::data_dir(),
            &live_pending_delivery_digests,
        )
        .expect("pending delivery payload store init"),
    );

    let user_last_activity = crate::user_last_activity::UserLastActivity::new();
    states.mesh.lock().build_hash = codeterm_build_info::hash().to_string();
    states.mesh.lock().last_activity = Arc::new(crate::mesh::UserLastActivityProvider(
        user_last_activity.clone(),
    ));
    let workers = crate::mem::workers::GraphWorkerRegistry::load(
        crate::paths::data_dir().join("mem").join("workers.json"),
    );
    let remote_workers = crate::service::remote_workers::RemoteWorkerRegistry::load(
        crate::paths::data_dir().join("remote_workers.json"),
    );
    let delayed_send_path = crate::paths::data_dir().join("delayed_send.json");
    let delayed_send =
        crate::delayed_send::DelayedSendQueue::open(delayed_send_path.clone())
            .unwrap_or_else(|error| {
                log::error!(
                    "[delayed_send] startup read failed for {}: {}; continuing with an empty in-memory queue",
                    delayed_send_path.display(),
                    error
                );
                crate::delayed_send::DelayedSendQueue::empty(delayed_send_path)
            });
    let startup_config = states.config.lock().clone();
    let kpi =
        crate::agent::kpi::KpiRecorder::from_config(&startup_config, &crate::paths::data_dir());
    {
        let handle = app_handle.clone();
        kpi.on_commit(move |plan_ids| {
            codeterm_events::emit_everywhere(
                &handle,
                "kpi-recorded",
                serde_json::json!({ "plan_ids": plan_ids }),
            );
        });
    }
    let secrets = crate::secrets::new_state(&startup_config.integrations.secrets, &secret_registry);
    let services_store =
        crate::service::store::ServicesStore::open(&crate::paths::data_dir().join("services.db"))
            .expect("services.db init");
    let usage_store =
        crate::usage::UsageStore::open(&crate::paths::data_dir()).expect("usage.db init");

    let mut extensions = crate::service::extensions::ExtensionsBuilder::default();
    extensions.insert(crate::service::workspace_cwd::WorkspaceRelocationLock(
        Arc::new(parking_lot::Mutex::new(())),
    ));
    extensions.insert(crate::service::account::AccountCell(Arc::new(
        tokio::sync::OnceCell::new(),
    )));
    extensions.insert(crate::service::billing::BillingCell(Arc::new(
        tokio::sync::OnceCell::new(),
    )));
    extensions.insert(crate::service::analytics::AnalyticsCell(Arc::new(
        tokio::sync::OnceCell::new(),
    )));
    extensions.insert(crate::service::task_files::LaunchTaskFiles(Arc::new(
        parking_lot::Mutex::new(std::collections::HashMap::new()),
    )));
    extensions.insert(crate::service::CartographerAttachLocks(Default::default()));
    extensions.insert(crate::service::OrchestratorAttachLocks(Default::default()));
    extensions.insert(crate::ports::orchestration::InstalledOrchestration::default());
    extensions.insert::<crate::ShellCapabilityState>(Arc::new(parking_lot::Mutex::new(
        Default::default(),
    )));
    extensions.insert::<Arc<crate::service::protocol_recorder::ProtocolRecorder>>(Arc::new(
        crate::service::protocol_recorder::ProtocolRecorder::new(
            crate::paths::data_dir().join("logs/protocol"),
            crate::paths::is_dev_instance(),
        ),
    ));
    extensions.insert::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>(Arc::new(
        Default::default(),
    ));
    extensions.insert::<Arc<crate::mem::supervisor::Supervisor>>(mem_supervisor);
    extensions.insert::<Arc<crate::plugin::activity::PluginActivityRegistry>>(Arc::new(
        crate::plugin::activity::PluginActivityRegistry::new(),
    ));
    extensions.insert::<Arc<crate::mem::worker_pool::WorkerPool>>(Arc::new(
        crate::mem::worker_pool::WorkerPool::new(
            crate::config::memory_config().workers_per_provider,
        ),
    ));
    extensions.insert::<Arc<crate::service::process_tree::ProviderTreeWitnesses>>(Arc::new(
        Default::default(),
    ));
    extensions.insert(crate::service::BridgeOutboundRx(Arc::new(Mutex::new(
        Some(bridge_outbound_rx),
    ))));
    extensions.insert::<Arc<crate::bridge::supervisor::BridgeSupervisor>>(Arc::new(
        bridge::supervisor::BridgeSupervisor::new(),
    ));
    extensions.insert::<Arc<codeterm_bridge::bridge::threads::ThreadService>>(Arc::new(
        codeterm_bridge::bridge::threads::ThreadService::new(
            codeterm_bridge::bridge::threads::ThreadRegistry::load(
                crate::service::bridge_threads::registry_path(),
            ),
        ),
    ));
    extensions.insert::<crate::service::agent_restart::RestartTracker>(
        crate::service::agent_restart::new_restart_tracker(),
    );
    extensions.insert::<crate::delayed_send::DelayedSendQueue>(delayed_send);
    extensions.insert::<Arc<crate::secrets::SecretRegistry>>(secret_registry.clone());
    extensions.insert::<Arc<crate::transcribe::TranscribeRegistry>>(transcribe_registry.clone());
    extensions
        .insert::<Arc<codeterm_plugin::converter::ConverterRegistry>>(converter_registry.clone());
    extensions.insert::<Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>(
        graph_node_registry.clone(),
    );
    extensions.insert::<Arc<codeterm_plugin::event_source::EventSourceRegistry>>(
        event_source_registry.clone(),
    );
    extensions.insert::<codeterm_plugin::graph_events::SubscriptionTable>(
        graph_event_subscriptions.clone(),
    );
    extensions.insert::<crate::glance::GlanceRegistryHandle>(glance_registry.clone());
    extensions.insert::<crate::bubble::BubbleRegistryHandle>(bubble_registry.clone());
    extensions.insert::<crate::repowatch::RepoWatchRegistryHandle>(repo_watch);
    extensions.insert::<crate::view_host::ViewRegistryHandle>(view_registry.clone());
    extensions.insert::<Arc<crate::service::tab_transport_chat::TabReopenGuards>>(Arc::new(
        Default::default(),
    ));
    extensions.insert::<crate::chatpreprocessor::ChatPreprocessorRegistryHandle>(
        chat_preprocessor_registry.clone(),
    );
    extensions.insert::<crate::agent_commands::AgentCommandRegistryHandle>(
        agent_command_registry.clone(),
    );
    extensions.insert::<codeterm_plugin::WebhookRegistryHandle>(webhook_registry.clone());
    extensions.insert::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>(
        cap_registries.local_recording.clone(),
    );
    extensions.insert::<Arc<crate::service::local_recording::RecordingSelectionStore>>(Arc::new(
        crate::service::local_recording::RecordingSelectionStore::open(
            crate::paths::data_dir().join("local-recording-selections.json"),
        ),
    ));
    extensions.insert(crate::service::receiver_target::LocalRecordingTargets(
        Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(
            crate::paths::data_dir().join("local-recording-targets.json"),
        )),
    ));
    extensions
        .insert::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>(tab_menu_registry.clone());
    extensions.insert::<codeterm_plugin::surfaces::SurfaceRegistryHandle>(surface_registry);
    extensions.insert::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>(
        crate::service::surface::builtin_runtimes(),
    );
    extensions.insert::<Arc<crate::service::surface::SurfaceActivityGate>>(Default::default());
    extensions.insert(crate::service::receiver_target::ReceiverTargets(Arc::new(
        crate::service::receiver_target::ReceiverTargetStore::open(
            crate::service::receiver_target::ReceiverTargetStore::default_path(),
        ),
    )));
    extensions.insert(crate::service::MuxEmitPending(Arc::new(
        std::sync::atomic::AtomicBool::new(false),
    )));
    extensions.insert::<Arc<crate::service::MuxEmitStats>>(Arc::new(
        crate::service::MuxEmitStats::default(),
    ));
    extensions.insert(crate::service::PublishedTabStatuses(Arc::new(
        Default::default(),
    )));
    extensions.insert(
        crate::service::output_stream_activity::OutputStreamActivityTx(
            crate::service::output_stream_activity::channel(),
        ),
    );
    extensions
        .insert::<crate::pasteboard::PasteboardState>(crate::pasteboard::PasteboardState::new());
    extensions.insert::<Arc<crate::service::input_parser::InputParser>>(input_parser);
    extensions.insert::<crate::usage::TabActivity>(crate::usage::TabActivity::new());
    extensions.insert::<Arc<crate::user_last_activity::UserLastActivity>>(user_last_activity);
    extensions.insert(crate::usage::poller::UsagePollerSlot(Arc::new(
        parking_lot::Mutex::new(None),
    )));
    extensions.insert(crate::service::PlanWatchers(Arc::new(
        parking_lot::Mutex::new(std::collections::HashMap::new()),
    )));
    extensions.insert(crate::service::LoadedPlanProjects(Arc::new(
        parking_lot::Mutex::new(std::collections::HashSet::new()),
    )));
    extensions.insert(crate::service::PlanStartupLoaded(Arc::new(
        std::sync::atomic::AtomicBool::new(false),
    )));
    extensions.insert::<Arc<crate::service::hierarchy::stalled_team::StallEpisodeTracker>>(
        Arc::new(Default::default()),
    );
    extensions.insert::<Arc<crate::service::hierarchy::heartbeat_gate::Gate>>(Arc::new(
        Default::default(),
    ));
    extensions.insert(crate::service::WorkerIdentityAllocator(Arc::new(
        parking_lot::Mutex::new(crate::service::WorkerPersonaReservations::default()),
    )));
    let svc_ctx = ServiceContext {
        lifecycle_journal: crate::session::lifecycle::TabLifecycleJournal::shared_arc(),
        #[cfg(any(test, feature = "test-support"))]
        ready_user_send_tasks: Arc::new(service::test_fixture::TestReadySendTasks::default()),
        agent_pad_data_dir: crate::paths::data_dir(),
        mux: states.mux.clone(),
        pty: states.pty.clone(),
        output_buf: states.output_buffer.clone(),
        screen_state: states.screen_state.clone(),
        notif: states.notif.clone(),
        events: states.event_broadcaster.clone(),
        app_handle: app_handle.clone(),
        agent_tracker: states.agent_tracker.clone(),
        report_store: states.report_store.clone(),
        kpi,
        plan_store,
        plan_turn_counter,
        pending_delivery_payloads,
        chat_send_ledger: Arc::new(
            crate::service::chat_send_transaction::ChatSendLedger::default(),
        ),
        deferred_session_bindings: Arc::new(Default::default()),
        agent_registry,
        mem,
        activity: crate::mem::activity::ActivityRing::new(500),
        workers,
        prompt_detector,
        pty_ownership: Default::default(),
        agent_run_pty_map: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
        run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
        remote_workers: Arc::new(remote_workers),
        bridge_outbound: bridge_outbound_tx,
        chat_registry: crate::service::tab_chat::ChatRegistry::new(),
        secrets,
        supporter_models: decision_model_registry,
        chatbackend_registry: chatbackend_registry.clone(),
        provider_registry,
        local_recordings: Arc::new(crate::service::local_recording::RecordingLeases::default()),
        services_store: Arc::new(services_store),
        tokens: Arc::new(crate::service::tokens::TokenStore::new()),
        usage_store,
        worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
        gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
        extensions: Arc::new(extensions.finish()),
    };
    svc_ctx
}
