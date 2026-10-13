use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::Manager;

use super::ServiceContext;
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::PlanStore;

pub(crate) struct ServiceFixture {
    pub(crate) service: ServiceContext,
    /// The waiter lifetime owner. The registry keeps only a `Weak` of this, so
    /// dropping the fixture is itself the teardown signal.
    _owner: Arc<ServiceContext>,
    pub(crate) _app: crate::app_runtime::App,
    pub(crate) temp: tempfile::TempDir,
}

#[derive(Default)]
pub(crate) struct TestReadySendTasks {
    pending: Mutex<HashMap<String, Vec<tokio::sync::oneshot::Receiver<bool>>>>,
}

pub(crate) struct TestReadySendTask(Option<tokio::sync::oneshot::Sender<bool>>);

impl Drop for TestReadySendTask {
    fn drop(&mut self) {
        if let Some(completion) = self.0.take() {
            let _ = completion.send(!std::thread::panicking());
        }
    }
}

impl TestReadySendTasks {
    pub(crate) fn start(&self, tab_id: &str) -> TestReadySendTask {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending
            .lock()
            .entry(tab_id.to_owned())
            .or_default()
            .push(rx);
        TestReadySendTask(Some(tx))
    }

    pub(crate) fn wait(&self, tab_id: &str, deadline: std::time::Duration) {
        let pending = self.pending.lock().remove(tab_id).unwrap_or_default();
        tauri::async_runtime::block_on(async {
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

pub(crate) struct TestPtyGuard {
    pty: Arc<TimedMutex<crate::pty::manager::PtyManager>>,
    pty_id: String,
}

impl Drop for TestPtyGuard {
    fn drop(&mut self) {
        let _ = self.pty.lock().kill(&self.pty_id);
    }
}

#[derive(Default)]
pub(crate) struct RecordingHierarchyReportDelivery {
    pub(crate) writes: Mutex<Vec<(String, String)>>,
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
pub(crate) struct FailFirstHierarchyReportDelivery {
    pub(crate) attempts: Mutex<Vec<(String, String)>>,
    pub(crate) writes: Mutex<Vec<(String, String)>>,
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
    pub(crate) fn install_seeded_storage_desk(&self) -> crate::storage::Storage {
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
                .find(|entry| entry.id == workspace.id)
                .expect("current mux workspace is persisted");
            assert_eq!(persisted.tabs.len(), workspace.tabs.len());
            for tab in &workspace.tabs {
                assert!(persisted.tabs.iter().any(|entry| entry.id == tab.id));
            }
        }
        super::session_persist::install_storage_desk_adapter(self._app.handle(), adapter)
            .expect("install seeded fixture storage desk");
        storage
    }

    pub(crate) fn set_agent_full_access(&self, full_access: bool) {
        self._app
            .state::<crate::config::ConfigState>()
            .lock()
            .agent
            .full_access = Some(full_access);
    }

    pub(crate) fn new(mux: MultiplexerState) -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self::build(mux, temp, None)
    }

    pub(crate) fn new_at(mux: MultiplexerState, data_dir: &std::path::Path) -> Self {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(data_dir).unwrap();
        Self::build(mux, temp, Some(data_dir.to_path_buf()))
    }

    pub(crate) fn bind_test_pty_session(
        &self,
        tab_id: &str,
        session_id: &str,
        binding_nonce: &str,
    ) -> TestPtyGuard {
        let (pty_id, _) = self
            .service
            .pty
            .lock()
            .spawn(None, None, 80, 24, HashMap::new())
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
        let builder = crate::app_runtime::builder();
        let app = builder
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let mut config = crate::config::schema::AppConfig::default();
        config.agent.full_access = Some(false);
        let config: crate::config::ConfigState = Arc::new(Mutex::new(config));
        assert!(app.manage(config));
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
        let service = ServiceContext {
            lifecycle_journal: Arc::new(crate::session::lifecycle::TabLifecycleJournal::for_dir(
                data_dir.join("sessions"),
            )),
            ready_user_send_tasks: Arc::new(TestReadySendTasks::default()),
            workspace_relocation_lock: Arc::new(parking_lot::Mutex::new(())),
            account: Arc::new(tokio::sync::OnceCell::new()),
            billing: Arc::new(tokio::sync::OnceCell::new()),
            analytics: Arc::new(tokio::sync::OnceCell::new()),
            agent_pad_data_dir: data_dir.clone(),
            launch_task_files: Arc::new(Mutex::new(HashMap::new())),
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
            notif: Arc::new(Mutex::new(if persistent {
                crate::notification::store::NotificationStore::open_persisted(
                    data_dir.join("notifications.json"),
                    10,
                )
            } else {
                crate::notification::store::NotificationStore::new(10)
            })),
            events: Arc::new(crate::api::events::EventBroadcaster::new()),
            app_handle: app.handle().clone(),
            cartographer_attach_locks: Default::default(),
            orchestrator_attach_locks: Default::default(),
            agent_tracker: Arc::new(TimedMutex::new(
                "agent_tracker",
                crate::agent::status::TabStatusTracker::new(),
            )),
            shell_caps: Arc::new(Mutex::new(Default::default())),
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
            protocol_recorder: Arc::new(crate::service::protocol_recorder::ProtocolRecorder::new(
                temp.path().join("logs/protocol"),
                false,
            )),
            agent_registry: agent_registry.clone(),
            mem,
            mem_supervisor,
            activity: crate::mem::activity::ActivityRing::new(10),
            workers: crate::mem::workers::GraphWorkerRegistry::load(data_dir.join("workers.json")),
            plugin_activity: Arc::new(crate::plugin::activity::PluginActivityRegistry::new()),
            worker_pool: Arc::new(crate::mem::worker_pool::WorkerPool::new(1)),
            prompt_detector: Arc::new(Mutex::new(crate::agent::prompt::PromptDetector::new())),
            pty_ownership: Default::default(),
            provider_tree_witnesses: Arc::new(Default::default()),
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
            bridge_outbound_rx: Arc::new(Mutex::new(Some(bridge_outbound_rx))),
            bridge_supervisor: Arc::new(crate::bridge::supervisor::BridgeSupervisor::new()),
            bridge_threads: Arc::new(codeterm_bridge::bridge::threads::ThreadService::new(
                codeterm_bridge::bridge::threads::ThreadRegistry::in_memory(),
            )),
            restart_tracker: crate::service::agent_restart::new_restart_tracker(),
            chat_registry: crate::service::tab_chat::ChatRegistry::new(),
            delayed_send: crate::delayed_send::DelayedSendQueue::open(
                data_dir.join("delayed_send.json"),
            )
            .unwrap(),
            secrets,
            secret_registry,
            transcribe_registry: Arc::new(crate::transcribe::TranscribeRegistry::new()),
            converter_registry: Arc::new(codeterm_plugin::converter::ConverterRegistry::new()),
            graph_node_registry: Arc::new(
                codeterm_plugin::graph_nodes::GraphNodeRegistry::default(),
            ),
            event_source_registry: Arc::new(
                codeterm_plugin::event_source::EventSourceRegistry::default(),
            ),
            graph_event_subscriptions: codeterm_plugin::graph_events::SubscriptionTable::new(),
            supporter_models: Arc::new(crate::api::mem_routes::decision::SupporterRegistry::new()),
            glance_registry: Arc::new(crate::glance::GlanceRegistry::new()),
            bubble_registry: Arc::new(crate::bubble::BubbleRegistry::new()),
            repo_watch: Arc::new(crate::repowatch::RepoWatchRegistry::new(Arc::new(
                crate::repowatch::TauriRepoEventSink::new(app.handle().clone()),
            ))),
            view_registry: Arc::new(crate::view_host::ViewRegistry::new()),
            chatbackend_registry,
            chatbackend_reopen_guards: Arc::new(Default::default()),
            provider_registry,
            chat_preprocessor_registry: Arc::new(
                crate::chatpreprocessor::ChatPreprocessorRegistry::new(),
            ),
            agent_command_registry: Arc::new(crate::agent_commands::AgentCommandRegistry::new()),
            webhook_registry: Arc::new(codeterm_plugin::WebhookRegistry::new()),
            local_recording_registry: Arc::new(
                codeterm_plugin::local_recording::LocalRecordingRegistry::default(),
            ),
            local_recordings: Arc::new(crate::service::local_recording::RecordingLeases::default()),
            local_recording_selections: Arc::new(
                crate::service::local_recording::RecordingSelectionStore::open(
                    data_dir.join("local-recording-selections.json"),
                ),
            ),
            local_recording_targets: Arc::new(
                crate::service::receiver_target::ReceiverTargetStore::open(
                    data_dir.join("local-recording-targets.json"),
                ),
            ),
            tab_menu_registry: Arc::new(codeterm_plugin::tab_menu::TabMenuRegistry::new()),
            surface_registry: Arc::new(codeterm_plugin::surfaces::SurfaceRegistry::new()),
            surface_runtimes: Arc::new(
                super::surface::SurfaceRuntimeRegistry::with_builtin_runtimes(&data_dir),
            ),
            surface_activity: Default::default(),
            receiver_targets: Arc::new(crate::service::receiver_target::ReceiverTargetStore::open(
                data_dir.join("receiver-targets.json"),
            )),
            mux_emit_pending: Arc::new(AtomicBool::new(false)),
            mux_emit_stats: Arc::new(crate::service::MuxEmitStats::default()),
            published_tab_statuses: Arc::new(Default::default()),
            output_stream_activity: super::output_stream_activity::channel(),
            session_persist_pending: Arc::new(AtomicBool::new(false)),
            services_store,
            tokens: Arc::new(crate::service::tokens::TokenStore::new_with_root(
                data_dir.join("tokens"),
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
            stalled_team_nudges: Arc::new(Default::default()),
            heartbeat_gate: Arc::new(Default::default()),
            pending_delivery_payloads,
            chat_send_ledger: Arc::new(Default::default()),
            reply_relays: Arc::new(Default::default()),
            deferred_session_bindings: Arc::new(Default::default()),
            worker_identity_allocator: Arc::new(Mutex::new(Default::default())),
            worker_ack: Arc::new(crate::service::worker_ack::WorkerAckRegistry::default()),
            gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
        };
        assert!(app.manage(Arc::clone(&service.mux)));
        let owner = Arc::new(service.clone());
        Self {
            service,
            _owner: owner,
            _app: app,
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
    pub(crate) fn seed_prompt_notification(
        &self,
        title: String,
        body: Option<String>,
        _severity: Option<&str>,
        actions: Option<Vec<codeterm_types::notification::NotificationAction>>,
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
    use tauri::{Emitter, Listener};

    #[test]
    fn fixture_constructs_emits_and_drops_on_an_owned_thread_without_windows() {
        std::thread::spawn(|| {
            let (mux, _) = MultiplexerState::new();
            let fixture = ServiceFixture::new(mux);
            assert!(fixture._app.webview_windows().is_empty());
            fixture.set_agent_full_access(true);
            let config = fixture
                .service
                .app_handle
                .state::<crate::config::ConfigState>();
            assert_eq!(config.lock().agent.full_access, Some(true));
            let (tx, rx) = std::sync::mpsc::channel();
            let listener = fixture
                .service
                .app_handle
                .listen("fixture-event", move |event| {
                    tx.send(event.payload().to_owned()).unwrap();
                });
            fixture
                .service
                .app_handle
                .emit("fixture-event", 42)
                .unwrap();
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap(),
                "42"
            );
            fixture.service.app_handle.unlisten(listener);
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
                fixture._app.handle(),
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
        super::super::session_persist::stop_worker(fixture._app.handle()).unwrap();
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
