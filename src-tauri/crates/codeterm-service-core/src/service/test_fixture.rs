use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::Mutex;

use super::ServiceContext;
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::PlanStore;

pub fn install_test_runtime() {
    let _ = crate::async_runtime::handle();
}

pub struct ServiceFixture {
    pub service: ServiceContext,
    pub ui: Arc<RecordedUiEvents>,
    /// The waiter lifetime owner. The registry keeps only a `Weak` of this, so
    /// dropping the fixture is itself the teardown signal.
    _owner: Arc<ServiceContext>,
    pub temp: tempfile::TempDir,
}

type UiListener = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;

#[derive(Default)]
pub struct RecordedUiEvents(
    pub Mutex<Vec<(String, serde_json::Value)>>,
    Mutex<Vec<(usize, String, UiListener)>>,
    std::sync::atomic::AtomicUsize,
    Option<Arc<codeterm_events::EventBroadcaster>>,
);

impl RecordedUiEvents {
    pub fn with_broadcaster(events: Arc<codeterm_events::EventBroadcaster>) -> Self {
        Self(
            Default::default(),
            Default::default(),
            Default::default(),
            Some(events),
        )
    }

    pub fn listen(
        &self,
        event: &str,
        callback: impl Fn(&serde_json::Value) + Send + Sync + 'static,
    ) -> usize {
        let id = self.2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.1
            .lock()
            .push((id, event.to_owned(), Arc::new(callback)));
        id
    }

    pub fn unlisten(&self, id: usize) {
        self.1.lock().retain(|(key, _, _)| *key != id);
    }
}

impl RecordedUiEvents {
    fn record(&self, event: &str, payload: serde_json::Value) {
        self.0.lock().push((event.to_owned(), payload.clone()));
        let listeners: Vec<_> = self
            .1
            .lock()
            .iter()
            .filter(|(_, name, _)| name == event)
            .map(|(_, _, listener)| listener.clone())
            .collect();
        for listener in listeners {
            listener(&payload);
        }
    }
}

impl crate::ports::host::UiEvents for RecordedUiEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        self.record(event, payload.clone());
        if let Some(events) = &self.3 {
            events.publish_generic(
                event,
                codeterm_events::web_safe_event_payload(event, payload),
            );
        }
    }

    fn emit_native(&self, event: &str, payload: serde_json::Value) {
        self.record(event, payload);
    }

    fn emit_to(&self, _: &str, event: &str, payload: serde_json::Value) {
        self.record(event, payload);
    }
}

#[derive(Default)]
pub struct TestReadySendTasks {
    pending: Mutex<HashMap<String, Vec<tokio::sync::oneshot::Receiver<bool>>>>,
}

pub struct TestReadySendTask(Option<tokio::sync::oneshot::Sender<bool>>);

impl Drop for TestReadySendTask {
    fn drop(&mut self) {
        if let Some(completion) = self.0.take() {
            let _ = completion.send(!std::thread::panicking());
        }
    }
}

impl TestReadySendTasks {
    pub fn start(&self, tab_id: &str) -> TestReadySendTask {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending
            .lock()
            .entry(tab_id.to_owned())
            .or_default()
            .push(rx);
        TestReadySendTask(Some(tx))
    }

    pub fn wait(&self, tab_id: &str, deadline: std::time::Duration) {
        let pending = self.pending.lock().remove(tab_id).unwrap_or_default();
        crate::async_runtime::block_on(async {
            tokio::time::timeout(deadline, async {
                for task in pending {
                    assert!(
                        task.await.expect("ready-send task reports completion"),
                        "ready-send task panicked"
                    );
                }
            })
            .await
            .expect("ready-send tasks complete before the failure deadline");
        });
    }
}

pub struct TestPtyGuard {
    pty: Arc<TimedMutex<crate::pty::manager::PtyManager>>,
    pty_id: String,
}

impl Drop for TestPtyGuard {
    fn drop(&mut self) {
        let _ = self.pty.lock().kill(&self.pty_id);
    }
}

/// Authenticated agent submissions captured at the PTY boundary, in order.
pub struct RecordedAgentDeliveries {
    _boundary: super::agent_delivery_outbox::test_boundary::Boundary,
    pub texts: Arc<Mutex<Vec<String>>>,
}

#[derive(Default)]
pub struct RecordingHierarchyReportDelivery {
    pub writes: Mutex<Vec<(String, String)>>,
}

impl super::hierarchy::HierarchyReportDelivery for RecordingHierarchyReportDelivery {
    fn deliver(
        &self,
        recipient_tab_id: &str,
        report: &crate::agent::reports::AgentReport,
    ) -> Result<bool, String> {
        self.writes
            .lock()
            .push((recipient_tab_id.to_string(), report.id.clone()));
        Ok(true)
    }
}

#[derive(Default)]
pub struct FailFirstHierarchyReportDelivery {
    pub attempts: Mutex<Vec<(String, String)>>,
    pub writes: Mutex<Vec<(String, String)>>,
}

impl super::hierarchy::HierarchyReportDelivery for FailFirstHierarchyReportDelivery {
    fn deliver(
        &self,
        recipient_tab_id: &str,
        report: &crate::agent::reports::AgentReport,
    ) -> Result<bool, String> {
        let write = (recipient_tab_id.to_string(), report.id.clone());
        let fail = {
            let mut attempts = self.attempts.lock();
            attempts.push(write.clone());
            attempts.len() == 1
        };
        if fail {
            return Err("delivery deferred".into());
        }
        self.writes.lock().push(write);
        Ok(true)
    }
}

impl ServiceFixture {
    /// Makes `tab_id` a live, session-bound agent recipient whose submissions are recorded,
    /// because a bare shell recipient waits out the submit deadlines for turn evidence.
    pub fn bind_recorded_agent_recipient(
        &self,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
    ) -> RecordedAgentDeliveries {
        {
            let mut mux = self.service.mux.write();
            mux.link_pty(tab_id, format!("recorded-{tab_id}"));
            let tab = mux.get_tab_mut(tab_id).expect("recipient tab");
            tab.agent_type.get_or_insert_with(|| "alpha".into());
            tab.backing = crate::multiplexer::types::TabBacking::Pty;
            tab.agent_booting = false;
            tab.agent_session_id = Some(session_id.to_string());
            tab.session_binding_nonce = Some(binding_nonce.to_string());
            tab.reported_session_binding_nonce = Some(binding_nonce.to_string());
            tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        }
        self.record_agent_deliveries(tab_id)
    }

    pub fn record_agent_deliveries(&self, tab_id: &str) -> RecordedAgentDeliveries {
        let texts = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&texts);
        let boundary = super::agent_delivery_outbox::test_boundary::Boundary::install(
            tab_id,
            Arc::new(move |payload| {
                let text = match &payload.transport {
                    super::messenger::QueuedAgentPayload::Text { text } => text.clone(),
                    super::messenger::QueuedAgentPayload::RoutedFrame { body, .. } => body.clone(),
                };
                captured.lock().push(text);
                Ok(())
            }),
            Arc::new(|| {}),
        );
        RecordedAgentDeliveries {
            _boundary: boundary,
            texts,
        }
    }

    pub fn install_seeded_storage_desk(&self) -> crate::storage::Storage {
        let storage = crate::storage::Storage::open(
            &self.temp.path().join("runtime-desk"),
            crate::storage::StorageConfig::default(),
        )
        .expect("open fixture storage desk");
        let snapshot = self.service.mux.read().to_session_snapshot();
        let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage.clone());
        adapter
            .import_snapshot(&snapshot, Vec::new())
            .expect("seed fixture storage desk from current mux");
        let state = adapter.state().expect("read seeded fixture storage desk");
        assert_eq!(state.snapshot.workspaces.len(), snapshot.workspaces.len());
        for workspace in &snapshot.workspaces {
            let persisted = state
                .snapshot
                .workspaces
                .iter()
                .find(|entry| entry.record.id.as_str() == workspace.id)
                .expect("current mux workspace is persisted");
            assert_eq!(persisted.tabs.len(), workspace.tabs.len());
            for tab in &workspace.tabs {
                assert!(persisted.tabs.iter().any(|entry| entry.id == tab.id));
            }
        }
        super::session_persist::install_storage_desk_adapter(&self.service, adapter)
            .expect("install seeded fixture storage desk");
        storage
    }

    pub fn set_agent_full_access(&self, full_access: bool) {
        self.service
            .ext::<crate::config::ConfigState>()
            .lock()
            .agent
            .full_access = Some(full_access);
    }

    pub fn new(mux: MultiplexerState) -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self::build(mux, temp, None)
    }

    pub fn new_at(mux: MultiplexerState, data_dir: &std::path::Path) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(data_dir).unwrap();
        Self::build(mux, temp, Some(data_dir.to_path_buf()))
    }

    pub fn bind_test_pty_session(
        &self,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
    ) -> TestPtyGuard {
        let (pty_id, _) = {
            let gate = self.service.pty.lock().spawn_gate();
            crate::pty::manager::build_session(gate, None, None, 80, 24, HashMap::new())
                .map(|built| self.service.pty.lock().register_session(None, built))
        }
        .expect("test PTY");
        {
            let mut mux = self.service.mux.write();
            mux.link_pty(tab_id, pty_id.clone());
            let tab = mux.get_tab_mut(tab_id).expect("test tab");
            tab.backing = crate::multiplexer::types::TabBacking::Pty;
            tab.agent_session_id = Some(session_id.to_string());
            tab.session_binding_nonce = Some(binding_nonce.to_string());
            tab.reported_session_binding_nonce = Some(binding_nonce.to_string());
            tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        }
        self.service
            .pty
            .lock()
            .rearm_guarded_input(&pty_id)
            .expect("test PTY input permit");
        TestPtyGuard {
            pty: Arc::clone(&self.service.pty),
            pty_id,
        }
    }

    fn build(
        mut mux: MultiplexerState,
        temp: tempfile::TempDir,
        data_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let persistent = data_dir.is_some();
        let data_dir = data_dir.unwrap_or_else(|| temp.path().to_path_buf());
        if persistent {
            if let Ok(store) = crate::multiplexer::state::ClosedTabsStore::open(&data_dir) {
                mux.attach_closed_store(store);
            }
        }
        let mut config = crate::config::schema::AppConfig::default();
        config.agent.full_access = Some(false);
        let config: crate::config::ConfigState = Arc::new(Mutex::new(config));
        let agent_registry = Arc::new(crate::agent::registry::AgentRegistry::new());
        let chatbackend_registry = Arc::new(crate::chatbackend::ChatBackendRegistry::new());
        let provider_registry = Arc::new(crate::provider::ProviderRegistry::new(
            agent_registry.clone(),
            chatbackend_registry.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new_for_test(&data_dir));
        let plan_turn_counter = Arc::new(crate::service::plan::reminders::TurnCounter::new());
        plan_store.bind_turn_counter(plan_turn_counter.clone());
        let mem = if persistent {
            crate::mem::MemManager::open_offline(&data_dir.join("mem")).unwrap()
        } else {
            crate::mem::MemManager::open_in_memory().unwrap()
        };
        let mem_supervisor = Arc::new(crate::mem::supervisor::Supervisor::new(
            data_dir.join("codeterm"),
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
                &data_dir,
                &HashSet::new(),
            )
            .unwrap(),
        );
        let services_store = Arc::new(
            crate::service::store::ServicesStore::open(&data_dir.join("services.db")).unwrap(),
        );
        let usage_store = crate::usage::UsageStore::open(&data_dir).unwrap();
        let mux = Arc::new(TimedRwLock::new("mux", mux));
        let agent_tracker = Arc::new(TimedMutex::new(
            "agent_tracker",
            crate::agent::status::TabStatusTracker::new(),
        ));
        let mut extensions = crate::service::extensions::ExtensionsBuilder::default();
        extensions.insert_service_slots();
        extensions.insert(config);
        let (private_key, public_key) = crate::mesh::crown::generate_peer_keypair();
        extensions.insert::<crate::MeshStateHandle>(Arc::new(Mutex::new(
            crate::mesh::MeshManager::in_memory(
                crate::mesh::types::MeshIdentity {
                    instance_id: uuid::Uuid::new_v4().to_string(),
                    private_key: Some(private_key),
                    public_key: Some(public_key),
                },
                "fixture".into(),
                0,
            ),
        )));

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
            Mutex::new(HashMap::new()),
        )));
        crate::ports::orchestration::register_fixture_extensions(&mut extensions);
        extensions.insert(crate::ports::orchestration::InstalledOrchestration::default());
        extensions.insert::<crate::ShellCapabilityState>(Arc::new(Mutex::new(Default::default())));
        extensions.insert::<Arc<crate::service::protocol_recorder::ProtocolRecorder>>(Arc::new(
            crate::service::protocol_recorder::ProtocolRecorder::new(
                temp.path().join("logs/protocol"),
                false,
            ),
        ));
        extensions.insert::<Arc<crate::mem::supervisor::Supervisor>>(mem_supervisor);
        extensions.insert::<Arc<crate::plugin::activity::PluginActivityRegistry>>(Arc::new(
            crate::plugin::activity::PluginActivityRegistry::new(),
        ));
        extensions.insert::<Arc<crate::mem::worker_pool::WorkerPool>>(Arc::new(
            crate::mem::worker_pool::WorkerPool::new(1),
        ));
        extensions.insert::<Arc<crate::service::process_tree::ProviderTreeWitnesses>>(Arc::new(
            Default::default(),
        ));
        extensions.insert(crate::service::BridgeOutboundRx(Arc::new(Mutex::new(
            Some(bridge_outbound_rx),
        ))));
        extensions.insert::<Arc<crate::bridge::supervisor::BridgeSupervisor>>(Arc::new(
            crate::bridge::supervisor::BridgeSupervisor::new(),
        ));
        extensions.insert::<Arc<codeterm_bridge::bridge::threads::ThreadService>>(Arc::new(
            codeterm_bridge::bridge::threads::ThreadService::new(
                codeterm_bridge::bridge::threads::ThreadRegistry::in_memory(),
            ),
        ));
        extensions.insert::<crate::service::agent_restart::RestartTracker>(
            crate::service::agent_restart::new_restart_tracker(),
        );
        extensions.insert::<crate::delayed_send::DelayedSendQueue>(
            crate::delayed_send::DelayedSendQueue::open(data_dir.join("delayed_send.json"))
                .unwrap(),
        );
        extensions.insert::<Arc<crate::secrets::SecretRegistry>>(secret_registry);
        extensions.insert::<Arc<crate::transcribe::TranscribeRegistry>>(Arc::new(
            crate::transcribe::TranscribeRegistry::new(),
        ));
        extensions.insert::<Arc<codeterm_plugin::converter::ConverterRegistry>>(Arc::new(
            codeterm_plugin::converter::ConverterRegistry::new(),
        ));
        extensions.insert::<Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>(Arc::new(
            codeterm_plugin::graph_nodes::GraphNodeRegistry::default(),
        ));
        extensions.insert::<Arc<codeterm_plugin::event_source::EventSourceRegistry>>(Arc::new(
            codeterm_plugin::event_source::EventSourceRegistry::default(),
        ));
        extensions.insert::<codeterm_plugin::graph_events::SubscriptionTable>(
            codeterm_plugin::graph_events::SubscriptionTable::new(),
        );
        extensions.insert::<crate::glance::GlanceRegistryHandle>(Arc::new(
            crate::glance::GlanceRegistry::new(),
        ));
        extensions.insert::<crate::bubble::BubbleRegistryHandle>(Arc::new(
            crate::bubble::BubbleRegistry::new(),
        ));
        let events = Arc::new(codeterm_events::EventBroadcaster::new());
        let ui = Arc::new(RecordedUiEvents::with_broadcaster(events.clone()));
        let repo_sink = Arc::new(crate::repowatch::TauriRepoEventSink::new(ui.clone()));
        extensions.insert::<crate::repowatch::RepoWatchRegistryHandle>(Arc::new(
            crate::repowatch::RepoWatchRegistry::new(repo_sink.clone()),
        ));
        extensions.insert::<crate::view_host::ViewRegistryHandle>(Arc::new(
            crate::view_host::ViewRegistry::new(),
        ));
        extensions.insert::<Arc<crate::service::tab_transport_chat::TabReopenGuards>>(Arc::new(
            Default::default(),
        ));
        extensions.insert::<crate::chatpreprocessor::ChatPreprocessorRegistryHandle>(Arc::new(
            crate::chatpreprocessor::ChatPreprocessorRegistry::new(),
        ));
        extensions.insert::<crate::agent_commands::AgentCommandRegistryHandle>(Arc::new(
            crate::agent_commands::AgentCommandRegistry::new(),
        ));
        extensions.insert::<codeterm_plugin::WebhookRegistryHandle>(Arc::new(
            codeterm_plugin::WebhookRegistry::new(),
        ));
        extensions.insert::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>(
            Arc::new(codeterm_plugin::local_recording::LocalRecordingRegistry::default()),
        );
        extensions.insert::<Arc<crate::service::local_recording::RecordingSelectionStore>>(
            Arc::new(
                crate::service::local_recording::RecordingSelectionStore::open(
                    data_dir.join("local-recording-selections.json"),
                ),
            ),
        );
        extensions.insert(crate::service::receiver_target::LocalRecordingTargets(
            Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(
                data_dir.join("local-recording-targets.json"),
            )),
        ));
        extensions.insert::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>(Arc::new(
            codeterm_plugin::tab_menu::TabMenuRegistry::new(),
        ));
        extensions.insert::<codeterm_plugin::surfaces::SurfaceRegistryHandle>(Arc::new(
            codeterm_plugin::surfaces::SurfaceRegistry::new(),
        ));
        extensions.insert::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>(Arc::new(
            super::surface::SurfaceRuntimeRegistry::with_builtin_runtimes(&data_dir),
        ));
        extensions.insert::<Arc<crate::service::surface::SurfaceActivityGate>>(Default::default());
        extensions.insert(crate::service::receiver_target::ReceiverTargets(Arc::new(
            crate::service::receiver_target::ReceiverTargetStore::open(
                data_dir.join("receiver-targets.json"),
            ),
        )));
        extensions.insert(crate::service::mux_emitter::MuxEmitter::default());
        extensions.insert(crate::service::MuxEmitPending(Arc::new(AtomicBool::new(
            false,
        ))));
        extensions.insert::<Arc<crate::service::MuxEmitStats>>(Arc::new(
            crate::service::MuxEmitStats::default(),
        ));
        extensions.insert(crate::service::PublishedTabStatuses(Arc::new(
            Default::default(),
        )));
        let initial_mux = super::status_reconciliation::snapshot_with_reconciled_status(
            &mux,
            &agent_tracker,
            &agent_registry,
        );
        extensions.insert(Arc::new(
            super::PublishedMuxState::new(initial_mux.generation, &initial_mux)
                .expect("initial fixture mux snapshot serialization"),
        ));
        extensions.insert(
            crate::service::output_stream_activity::OutputStreamActivityTx(
                super::output_stream_activity::channel(),
            ),
        );
        extensions.insert::<crate::pasteboard::PasteboardState>(crate::pasteboard::PasteboardState::new());
        extensions.insert::<Arc<crate::service::input_parser::InputParser>>(Arc::new(
            crate::service::input_parser::InputParser::new(agent_registry.clone()),
        ));
        extensions.insert::<crate::usage::TabActivity>(crate::usage::TabActivity::new());
        extensions.insert::<Arc<crate::user_last_activity::UserLastActivity>>(
            crate::user_last_activity::UserLastActivity::new(),
        );
        extensions.insert(crate::usage::poller::UsagePollerSlot(Arc::new(Mutex::new(
            None,
        ))));
        extensions.insert(crate::service::PlanWatchers(Arc::new(Mutex::new(
            HashMap::new(),
        ))));
        extensions.insert(crate::service::LoadedPlanProjects(Arc::new(Mutex::new(
            HashSet::new(),
        ))));
        extensions.insert(crate::service::PlanStartupLoaded(Arc::new(
            AtomicBool::new(false),
        )));
        extensions.insert::<Arc<crate::service::hierarchy::stalled_team::StallEpisodeTracker>>(
            Arc::new(Default::default()),
        );
        extensions.insert::<Arc<crate::service::hierarchy::heartbeat_gate::Gate>>(Arc::new(
            Default::default(),
        ));
        extensions.insert::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>(Arc::new(
            Default::default(),
        ));
        extensions.insert(crate::service::WorkerIdentityAllocator(Arc::new(
            Mutex::new(Default::default()),
        )));
        install_test_runtime();
        let service = ServiceContext {
            lifecycle_journal: Arc::new(crate::session::lifecycle::TabLifecycleJournal::for_dir(
                data_dir.join("sessions"),
            )),
            ready_user_send_tasks: Arc::new(TestReadySendTasks::default()),
            agent_pad_data_dir: data_dir.clone(),
            mux,
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
            notif: Arc::new(Mutex::new(if persistent {
                crate::notification::store::NotificationStore::open_persisted(
                    data_dir.join("notifications.json"),
                    10,
                )
            } else {
                crate::notification::store::NotificationStore::new(10)
            })),
            events,
            ui: ui.clone(),
            shell: Arc::new(crate::ports::host::NoShell),
            rt: crate::async_runtime::handle().clone(),
            agent_tracker,
            report_store: Arc::new(Mutex::new(if persistent {
                crate::agent::reports::ReportStore::with_ledger(
                    10,
                    Arc::new(crate::agent::ledger::OrchestrationLedger::open(&data_dir).unwrap()),
                )
            } else {
                crate::agent::reports::ReportStore::new(10)
            })),
            kpi: if persistent {
                crate::agent::kpi::KpiRecorder::with_ledger(Arc::new(
                    crate::agent::kpi_ledger::KpiLedger::open(&data_dir).unwrap(),
                ))
            } else {
                crate::agent::kpi::KpiRecorder::disabled()
            },
            agent_registry: agent_registry.clone(),
            mem,
            activity: crate::mem::activity::ActivityRing::new(10),
            workers: crate::mem::workers::GraphWorkerRegistry::load(data_dir.join("workers.json")),
            prompt_detector: Arc::new(Mutex::new(crate::agent::prompt::PromptDetector::new())),
            pty_ownership: Default::default(),
            agent_run_pty_map: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new()),
            remote_workers: Arc::new(if persistent {
                crate::service::remote_workers::RemoteWorkerRegistry::load(
                    data_dir.join("remote_workers.json"),
                )
            } else {
                crate::service::remote_workers::RemoteWorkerRegistry::new()
            }),
            bridge_outbound,
            chat_registry: crate::service::tab_chat::ChatRegistry::new(),
            secrets,
            supporter_models: Arc::new(
                crate::service::supporter::registry::SupporterRegistry::new(),
            ),
            chatbackend_registry,
            provider_registry,
            local_recordings: Arc::new(crate::service::local_recording::RecordingLeases::default()),
            services_store,
            tokens: Arc::new(crate::service::tokens::TokenStore::new_with_root(
                data_dir.join("tokens"),
            )),
            usage_store,
            plan_store,
            plan_turn_counter,
            pending_delivery_payloads,
            chat_send_ledger: Arc::new(Default::default()),
            deferred_session_bindings: Arc::new(Default::default()),
            worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
            gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
            extensions: Arc::new(extensions.finish()),
            host_extensions: Default::default(),
        };
        let owner = Arc::new(service.clone());
        assert!(service.set_ext(Arc::downgrade(&owner)).is_ok());
        repo_sink.bind_service(Arc::downgrade(&owner));
        crate::ports::orchestration::install_fixture(&owner);
        Self {
            service,
            ui,
            _owner: owner,
            temp,
        }
    }
}

impl Drop for ServiceFixture {
    fn drop(&mut self) {
        let reapers = self.service.pty.lock().begin_shutdown().unwrap_or_default();
        for reaper in reapers {
            reaper.reap_in_background();
        }
    }
}

impl ServiceFixture {
    #[allow(clippy::too_many_arguments)]
    pub fn seed_prompt_notification(
        &self,
        title: String,
        body: Option<String>,
        _severity: Option<&str>,
        actions: Option<Vec<codeterm_notification_core::contract::NotificationAction>>,
        sticky: Option<bool>,
        origin: super::notification::NotificationOrigin,
        intent: super::notification::PushIntent,
    ) -> String {
        self.service.notif.lock().add_ext(
            title,
            body,
            crate::notification::store::NotificationSeverity::Warning,
            crate::notification::store::NotificationProvenance {
                tab_id: origin.caller_tab_id,
                ..Default::default()
            },
            crate::notification::store::Ask::offering(actions),
            sticky,
            intent.level,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_configuration_and_weak_owner_are_initialized() {
        let fixture = ServiceFixture::new(MultiplexerState::new().0);
        fixture.set_agent_full_access(true);
        assert_eq!(
            fixture.service.configuration().agent.full_access,
            Some(true)
        );
        let owner = fixture
            .service
            .ext::<std::sync::Weak<ServiceContext>>()
            .upgrade()
            .unwrap();
        assert!(Arc::ptr_eq(&owner, &fixture._owner));
    }

    #[test]
    fn fixture_mesh_identity_is_isolated_and_never_persists() {
        let first = ServiceFixture::new(MultiplexerState::new().0);
        let second = ServiceFixture::new(MultiplexerState::new().0);
        let first_mesh = first.service.ext::<crate::MeshStateHandle>();
        let second_mesh = second.service.ext::<crate::MeshStateHandle>();
        assert!(!Arc::ptr_eq(first_mesh, second_mesh));
        let first_mesh = first_mesh.lock();
        let second_mesh = second_mesh.lock();
        assert!(!first_mesh.disk_persist);
        assert!(!second_mesh.disk_persist);
        assert!(first_mesh.paired_peers.is_empty());
        assert!(second_mesh.paired_peers.is_empty());
        assert_ne!(
            first_mesh.ident.identity.instance_id,
            second_mesh.ident.identity.instance_id
        );
    }

    #[test]
    fn recorded_ui_preserves_event_payloads_in_order() {
        use crate::ports::host::UiEvents;
        let ui = RecordedUiEvents::default();
        let payload = serde_json::json!({ "probe": [1, 2] });
        ui.emit("broadcast", payload.clone());
        ui.emit_to("main", "targeted", serde_json::Value::Null);
        assert_eq!(
            *ui.0.lock(),
            vec![
                ("broadcast".to_owned(), payload),
                ("targeted".to_owned(), serde_json::Value::Null)
            ]
        );
    }

    #[test]
    fn recorded_ui_broadcasts_only_global_events() {
        use crate::ports::host::UiEvents;
        let events = Arc::new(codeterm_events::EventBroadcaster::new());
        let mut receiver = events.subscribe();
        let ui = RecordedUiEvents::with_broadcaster(events);
        let payload = serde_json::json!({"probe": [1, 2]});
        ui.emit("broadcast", payload.clone());
        match receiver.try_recv().unwrap().event {
            codeterm_events::SystemEvent::Generic { event_type, data } => {
                assert_eq!(event_type, "broadcast");
                assert_eq!(data, payload);
            }
            event => panic!("unexpected event: {event:?}"),
        }
        ui.emit_native("native", serde_json::Value::Null);
        ui.emit_to("main", "targeted", serde_json::Value::Null);
        assert!(receiver.try_recv().is_err());
        assert_eq!(ui.0.lock().len(), 3);
    }

    #[test]
    fn service_runtime_uses_the_host_pool_from_blocking_workers() {
        let fixture = ServiceFixture::new(MultiplexerState::new().0);
        let expected = fixture.service.rt.id();
        crate::async_runtime::block_on(async move {
            let spawned =
                crate::async_runtime::spawn(async { tokio::runtime::Handle::current().id() });
            assert_eq!(spawned.await.unwrap(), expected);
            let blocking = crate::async_runtime::spawn_blocking(|| {
                crate::async_runtime::block_on(async {
                    tokio::task::yield_now().await;
                    tokio::runtime::Handle::current().id()
                })
            });
            assert_eq!(blocking.await.unwrap(), expected);
        });
    }

    #[test]
    fn fixture_constructs_emits_and_drops_on_an_owned_thread_without_windows() {
        std::thread::spawn(|| {
            let (mux, _) = MultiplexerState::new();
            let fixture = ServiceFixture::new(mux);
            use crate::ports::host::UiEvents;
            fixture.set_agent_full_access(true);
            assert_eq!(
                fixture.service.configuration().agent.full_access,
                Some(true)
            );
            let (tx, rx) = std::sync::mpsc::channel();
            let listener = fixture.ui.listen("fixture-event", move |value| {
                tx.send(value.clone()).unwrap();
            });
            fixture.ui.emit("fixture-event", serde_json::json!(42));
            assert_eq!(rx.try_recv().unwrap(), serde_json::json!(42));
            fixture.ui.unlisten(listener);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn seeded_storage_desk_preserves_current_mux_and_subsequent_session_writes() {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
        let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
        let storage = fixture.install_seeded_storage_desk();
        let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage);
        let seeded = MultiplexerState::from_session_snapshot(&adapter.snapshot().unwrap());
        assert!(seeded.get_tab(&tab_id).is_some());
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_session_id = Some("supplied-thread".into());
            tab.session_binding_nonce = Some("supplied-nonce".into());
            tab.agent_runtime_config =
                Some(serde_json::json!({"systemPrompt":"supplied-identity"}));
        }
        assert!(matches!(
            super::super::session_persist::write_session_snapshot(
                &fixture.service,
                &fixture.service.mux,
            )
            .unwrap(),
            crate::session::store::SaveOutcome::Saved
        ));
        let persisted = MultiplexerState::from_session_snapshot(&adapter.snapshot().unwrap());
        let tab = persisted.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("supplied-thread"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("supplied-nonce"));
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["systemPrompt"],
            "supplied-identity"
        );
        super::super::session_persist::stop_worker(&fixture.service).unwrap();
    }

    #[test]
    fn fixture_journals_isolate_identical_tabs_through_compaction_and_inspection() {
        use crate::multiplexer::types::AgentRole;
        use crate::session::lifecycle::{JournalRecord, LifecycleTransition};
        let (mux, tab_id) = MultiplexerState::new();
        let snapshot = mux.to_session_snapshot();
        let first = ServiceFixture::new(mux);
        let second = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
        first.service.mux.write().get_tab_mut(&tab_id).unwrap().role = Some(AgentRole::Worker);
        {
            let mut mux = second.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.role = Some(AgentRole::Manager);
            tab.role_profile = Some("planner".into());
        }
        let first_key = first.service.open_lifecycle_epoch(&tab_id).unwrap();
        assert!(second.service.lifecycle_journal.load().records.is_empty());
        let second_key = second.service.open_lifecycle_epoch(&tab_id).unwrap();
        assert_ne!(first_key, second_key);
        let records = second.service.lifecycle_journal.load().records;
        assert!(records.iter().any(|record| matches!(record,
            JournalRecord::Transition { entry } if matches!(&entry.transition,
                LifecycleTransition::RoleBinding { role: Some(role), role_profile: Some(profile), .. }
                    if role == "Manager" && profile == "planner"
            )
        )));
        first.service.lifecycle_journal.compact().unwrap();
        assert_eq!(second.service.lifecycle_journal.load().records, records);
        let inspected = crate::dispatch::dispatch(
            &crate::dispatch::Command::TabInspect {
                tab_ids: vec![tab_id],
                lifecycle: true,
            },
            &second.service,
        )
        .unwrap()
        .into_json();
        assert_eq!(
            inspected[0]["lifecycle"]["transitions"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert!(Arc::ptr_eq(
            &second.service.lifecycle_journal,
            &second.service.clone().lifecycle_journal
        ));
    }
}
