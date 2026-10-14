use super::*;
use crate::agent::provider::*;
use crate::service::test_fixture::ServiceFixture;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

struct Adapter;

#[async_trait::async_trait]
impl AgentProvider for Adapter {
    fn id(&self) -> &str {
        "runtime-subscriber-fixture"
    }
    fn display_name(&self) -> &str {
        self.id()
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        SystemPromptDelivery::External
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn agent_runtime(&self) -> Option<AgentRuntimeCapability> {
        Some(AgentRuntimeCapability {
            usage: false,
            runtime: AgentRuntimeProtocol::StdioJsonrpc,
            command: AgentRuntimeCommand {
                binary: "unused".into(),
                args: Vec::new(),
            },
            inherited_environment_exclusions: Vec::new(),
            login: None,
            request_methods: std::collections::BTreeMap::from([(
                "control".into(),
                vec!["fixture/restore".into()],
            )]),
            notification_methods: vec!["fixture/compacted".into()],
        })
    }
    async fn runtime_plan(
        &self,
        _: &RuntimeContext,
        operation: &RuntimeOperation,
    ) -> Result<RuntimePlan, codeterm_plugin::agent_runtime::RuntimeFailure> {
        let RuntimeOperation::Control { action, value, .. } = operation else {
            panic!("unexpected operation")
        };
        assert_eq!(action, "restoreContext");
        Ok(RuntimePlan {
            action: RuntimeAction::Request {
                call: RpcCall {
                    target: None,
                    method: "fixture/restore".into(),
                    params: value.clone(),
                },
            },
            state: Value::Null,
        })
    }
    async fn runtime_response(
        &self,
        _: &RuntimeContext,
        _: &RuntimeOperation,
        _: &Value,
        reply: &RpcReply,
    ) -> Result<RuntimeStep, codeterm_plugin::agent_runtime::RuntimeFailure> {
        Ok(match reply {
            RpcReply::Success { .. } => RuntimeStep::Complete {
                result: RuntimeResult::Done,
                events: None,
                notification: None,
            },
            RpcReply::Error { .. } => RuntimeStep::Refused {
                reason: codeterm_plugin::agent_runtime::RuntimeFailure::new(
                    RuntimeFailureCode::ProviderRefused,
                    "fixture refusal",
                ),
                before_acceptance: true,
            },
        })
    }
    async fn runtime_reduce(
        &self,
        _: &RuntimeContext,
        _: &Value,
        _: &[RuntimeFrame],
    ) -> Result<RuntimeReduction, codeterm_plugin::agent_runtime::RuntimeFailure> {
        Ok(RuntimeReduction {
            state: Value::Null,
            events: Vec::new(),
        })
    }
}

pub(super) struct Scene {
    pub fixture: ServiceFixture,
    pub session: Arc<Session>,
    pub peer: BufReader<DuplexStream>,
}

pub(super) fn scene() -> Scene {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("Fixture".into(), None).id;
    let tab = mux.create_tab(&workspace).unwrap();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("runtime-subscriber-fixture".into());
        target.agent_session_id = Some("thread".into());
        target.session_binding_nonce = Some("nonce".into());
        target.agent_runtime_config = Some(json!({"systemPrompt":"opaque instructions"}));
        target.cwd = Some(
            crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
        );
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(Adapter));
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc("runtime-subscriber-fixture")
        .unwrap();
    let (host, peer) = tokio::io::duplex(65536);
    let (read, write) = tokio::io::split(host);
    let client = super::super::rpc::Client::attach(read, write);
    let process = Arc::new(Process::for_test(client));
    let mut state = super::super::model::RuntimeState::unloaded(Some("thread".into()));
    let epoch = state.begin();
    state.load(epoch, "thread");
    let session = Arc::new(Session {
        provider: "runtime-subscriber-fixture".into(),
        adapter: Mutex::new(Some(adapter)),
        context: Mutex::new(RuntimeContext {
            epoch,
            tab_id: tab,
            binding_nonce: "nonce".into(),
            thread_id: Some("thread".into()),
            cwd: crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
            host_version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        }),
        completions: Mutex::new(Default::default()),
        state: Mutex::new(state),
        cache: Mutex::new(Default::default()),
        login: Mutex::new(Default::default()),
        auth: Mutex::new(RuntimeAuthState::Unknown),
        thread_loaded: std::sync::atomic::AtomicBool::new(true),
        hook_state: Mutex::new(Value::Null),
        prompts: Mutex::new(HashMap::new()),
        process: Mutex::new(Some(process)),
        gate: tokio::sync::Semaphore::new(1),
        initialization: Default::default(),
        compaction: Mutex::new(Default::default()),
        abort_note: Mutex::new(None),
        controls_confirmation: Mutex::new(None),
        control_changes: tokio::sync::watch::channel(()).0,
    });
    validate_runtime_context(&session.context.lock()).unwrap();
    Scene {
        fixture,
        session,
        peer: BufReader::new(peer),
    }
}

pub(super) async fn read(peer: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_millis(500), peer.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

pub(super) async fn write(peer: &mut BufReader<DuplexStream>, value: Value) {
    peer.get_mut()
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
}
