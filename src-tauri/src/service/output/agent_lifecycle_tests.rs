use super::ServiceContext;
use crate::agent::offline_state_classifier::{TabVerdict, TimelineState};
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::plan::reconcile::TabProbe;
use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::boot_stage::BootStage;
use crate::service::hierarchy::clear_planner_bindings_for_lifecycle_exit;
use crate::service::plan::PlanStore;
use crate::service::tab::clear_agent_identity_for_lifecycle_exit;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const SHELL_SUFFIXES: [char; 4] = ['>', '$', '%', '#'];

struct OutputServiceHarness {
    service: ServiceContext,
    _app: tauri::App,
    _temp: tempfile::TempDir,
}

impl OutputServiceHarness {
    fn new(mux: MultiplexerState) -> Self {
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
            gateway: Arc::new(crate::service::message_gateway::MessageGateway::new()),
        };
        Self {
            service,
            _app: app,
            _temp: temp,
        }
    }
}

fn planner_probe(tab: &crate::multiplexer::types::Tab) -> TabProbe {
    let chat_delivery_target = tab.chat_view && tab.pty_id.is_some();
    let agent_accepting_work = chat_delivery_target
        && tab.agent_state.as_ref().is_some_and(|state| {
            matches!(state.state, TimelineState::Working | TimelineState::Idle)
        });
    TabProbe {
        exists: true,
        live_agent: tab.agent_type.is_some(),
        agent_session_id: tab.agent_session_id.clone(),
        session_binding_valid: tab.session_binding_nonce.is_some()
            && tab.session_binding_nonce == tab.reported_session_binding_nonce,
        chat_delivery_target,
        agent_accepting_work,
        role: None,
        parent_tab_id: None,
    }
}

fn seed_live_planner() -> (
    MultiplexerState,
    String,
    Option<PlannerRef>,
    OrchestratorRef,
) {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-planner".into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.agent_type_locked = true;
    tab.agent_session_id = Some("session-tab-owned".into());
    tab.session_binding_nonce = Some("launch-nonce-tab-owned".into());
    tab.reported_session_binding_nonce = Some("launch-nonce-tab-owned".into());
    tab.agent_session_model = Some("gpt-5.6".into());
    tab.role_profile = Some("planner".into());
    tab.chat_view = true;
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let planner = Some(PlannerRef::Tab {
        tab_id: tab_id.clone(),
        session_id: Some("session-tab-owned".into()),
        title: Some("Planner".into()),
    });
    (
        mux,
        tab_id,
        planner,
        OrchestratorRef::from_tab("orchestrator"),
    )
}

fn assert_live_planner_identity(tab: &crate::multiplexer::types::Tab) {
    assert_eq!(tab.agent_type.as_deref(), Some("codex"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("session-tab-owned"));
    assert_eq!(
        tab.session_binding_nonce.as_deref(),
        Some("launch-nonce-tab-owned")
    );
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("launch-nonce-tab-owned")
    );
    assert_eq!(tab.agent_session_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.agent_type_locked);
    assert!(tab
        .agent_state
        .as_ref()
        .is_some_and(|state| { matches!(state.state, TimelineState::Working) }));
}

fn assert_planner_resolves(
    tab_id: &str,
    planner: &Option<PlannerRef>,
    owner: &OrchestratorRef,
    tab: &crate::multiplexer::types::Tab,
) {
    assert!(matches!(
        resolve_planner_ref(planner, owner, |_| planner_probe(tab)),
        PeerResolution::Planner { tab_id: ref resolved } if resolved == tab_id
    ));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn planner_shell_like_live_tui_preserves_identity_across_snapshot_reload() {
    let (mux, tab_id, planner, owner) = seed_live_planner();
    let harness = OutputServiceHarness::new(mux);
    let plan = harness
        .service
        .plan_store
        .create_plan_for_team(
            "Live Planner".into(),
            harness._temp.path().display().to_string(),
            owner.clone(),
            "team-live-planner".into(),
            vec![],
            1,
        )
        .unwrap();
    harness
        .service
        .plan_store
        .set_planner_ref(&plan.id, planner, 2)
        .unwrap();

    for suffix in SHELL_SUFFIXES {
        let frame = format!(
            "\x1b[2J\x1b[H  GPT-5.6 Codex\r\n  Working in D:\\Developer\\codeterm\r\n\r\n› continue\r\nshell{suffix}"
        );
        {
            let mut screen_state = harness.service.screen_state.lock();
            screen_state.process("pty-planner", frame.as_bytes());
            assert!(super::screen_ends_in_shell_prompt(
                &screen_state.get_screen_text("pty-planner").unwrap()
            ));
        }
        harness
            .service
            .process_pty_output("pty-planner", frame.as_bytes());
        let stored_planner = harness.service.plan_store.get(&plan.id).unwrap().planner;
        let mux = harness.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_live_planner_identity(tab);
        assert_planner_resolves(&tab_id, &stored_planner, &owner, tab);
    }

    let mux = harness.service.mux.read();
    let snapshot = mux.to_session_snapshot();
    drop(mux);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    let restored_snapshot = serde_json::from_str(&serialized).unwrap();
    let mut restored = MultiplexerState::from_session_snapshot(&restored_snapshot);
    restored.link_pty(&tab_id, "pty-restored".into());
    let stored_planner = harness.service.plan_store.get(&plan.id).unwrap().planner;
    let tab = restored.get_tab(&tab_id).unwrap();
    assert_live_planner_identity(tab);
    assert_planner_resolves(&tab_id, &stored_planner, &owner, tab);
}

#[test]
fn explicit_lifecycle_exit_clears_planner_identity_once() {
    let (mut mux, tab_id, planner, owner) = seed_live_planner();
    let dir = tempfile::tempdir().unwrap();
    let plan_store = PlanStore::new_for_test(dir.path());
    let plan = plan_store
        .create_plan_for_team(
            "Planner lifecycle".into(),
            dir.path().display().to_string(),
            owner.clone(),
            "team-planner".into(),
            vec![],
            1,
        )
        .unwrap();
    plan_store
        .set_planner_ref(&plan.id, planner.clone(), 2)
        .unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();

    assert!(clear_agent_identity_for_lifecycle_exit(tab));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &tab_id, 3),
        1
    );
    assert!(!clear_agent_identity_for_lifecycle_exit(tab));
    assert_eq!(
        clear_planner_bindings_for_lifecycle_exit(&plan_store, &tab_id, 4),
        0
    );
    assert!(tab.agent_type.is_none());
    assert!(tab.agent_session_id.is_none());
    assert!(tab.session_binding_nonce.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
    assert!(tab.agent_state.is_none());
    assert_eq!(tab.boot_stage, Some(BootStage::ShellReady));
    assert!(matches!(
        plan_store.get(&plan.id).unwrap().planner,
        Some(PlannerRef::Tab {
            session_id: None,
            ..
        })
    ));
    assert!(matches!(
        resolve_planner_ref(&planner, &owner, |_| planner_probe(tab)),
        PeerResolution::OrchestratorFallback { .. }
    ));
}
