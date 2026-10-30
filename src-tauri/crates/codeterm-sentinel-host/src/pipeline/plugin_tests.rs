use super::*;
use crate::{blueprint_pipeline::BlueprintExt, store::Store};
use codeterm_plugin::{
    blueprint_step::*,
    graph_nodes::GraphNodeRegistry,
    view_host::{ViewCallContext, ViewError, ViewProvider, ViewRegistryHandle},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const TEXT: &str = "opaque text with quotes \" and a newline\n";
const BYTES: &[u8] = b"opaque file bytes\0\xff\n";

struct Adapter {
    service: std::sync::Weak<ServiceContext>,
    plugin: String,
    submits: AtomicUsize,
    reads: AtomicUsize,
}
impl BlueprintStep for Adapter {
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        let outputs = if call.request.node_type.ends_with(".produce") {
            [
                ("text".into(), vec![PortValue::Text { text: TEXT.into() }]),
                (
                    "json".into(),
                    vec![PortValue::Json {
                        value: json!({"n":7}),
                    }],
                ),
                (
                    "file".into(),
                    vec![PortValue::MediaSource {
                        id: "artifact".into(),
                        media: PortKind::File,
                    }],
                ),
            ]
            .into()
        } else {
            use codeterm_plugin::binary_transfer::*;
            let service = self.service.upgrade().unwrap();
            let PortValue::Media { handle, media } = &call.request.inputs["file"][0] else {
                panic!("file handle required")
            };
            assert_eq!(*media, PortKind::File);
            let _scope = StepInputScope::enter(&self.plugin, call, true);
            let context = current_step_input(&self.plugin).unwrap();
            let session =
                TransferSession::new(&self.plugin, Default::default(), Arc::new(InputAuthority))
                    .unwrap();
            activate(&session);
            let host = RunInputs(Arc::new(crate::plugin_run_inputs::Inputs(
                self.service.clone(),
            )));
            let grant = host
                .0
                .bind(
                    &self.plugin,
                    &context,
                    handle,
                    "https://fixture.invalid",
                    &InputPreparation {
                        abort: Default::default(),
                        deadline: call.deadline,
                    },
                )
                .unwrap();
            assert_eq!(grant.size, BYTES.len() as u64);
            assert_eq!(grant.sha256, format!("{:x}", Sha256::digest(BYTES)));
            let mut wrong = context.clone();
            wrong.request.context.owner = "foreign".into();
            assert!(host
                .0
                .bind(
                    &self.plugin,
                    &wrong,
                    handle,
                    "https://fixture.invalid",
                    &InputPreparation {
                        abort: Default::default(),
                        deadline: call.deadline,
                    }
                )
                .is_err());
            let mut wrong = context;
            wrong.request.context.scope.run = "foreign".into();
            assert!(host
                .0
                .bind(
                    &self.plugin,
                    &wrong,
                    handle,
                    "https://fixture.invalid",
                    &InputPreparation {
                        abort: Default::default(),
                        deadline: call.deadline,
                    }
                )
                .is_err());
            let source = plugin::media(&service)
                .unwrap()
                .resolve_input(&call.request.context, &self.plugin, handle, PortKind::File)
                .unwrap();
            let mut opened = RunMediaFiles {
                registry: service.ext::<ViewRegistryHandle>().clone(),
                runtime: service.rt.clone(),
            }
            .open(
                &source.lookup,
                &source.media_id,
                PortKind::File,
                Some(&source.fingerprint),
            )
            .unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut opened.file, &mut bytes).unwrap();
            assert_eq!(bytes, BYTES);
            self.reads.fetch_add(1, Ordering::SeqCst);
            deactivate(&self.plugin);
            PortValues::new()
        };
        Ok(JobUpdate {
            job: format!("job-{}", call.request.context.node),
            state: OperationState::Succeeded,
            outcome: Some("done".into()),
            outputs,
        })
    }
    fn reconcile(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("terminal fixture")
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("terminal fixture")
    }
}
struct InputAuthority;
impl codeterm_plugin::binary_transfer::TransferAuthority for InputAuthority {
    fn authorize(&self, url: &reqwest::Url) -> codeterm_plugin::binary_transfer::Result<()> {
        if url.origin().ascii_serialization() == "https://fixture.invalid" {
            Ok(())
        } else {
            Err(codeterm_plugin::binary_transfer::TransferRefusal::NotAllowed)
        }
    }
    fn credentials(
        &self,
        _: &codeterm_plugin::binary_transfer::Credential,
    ) -> codeterm_plugin::binary_transfer::Result<std::collections::BTreeMap<String, String>> {
        panic!("no network or credentials")
    }
    fn readable(&self, _: &Path) -> bool {
        false
    }
}
struct MediaProvider(String);
#[codeterm_plugin::async_trait]
impl ViewProvider for MediaProvider {
    fn id(&self) -> &str {
        &self.0
    }
    fn media_delivery(&self) -> Option<codeterm_plugin::media_delivery::MediaDeliveryDescriptor> {
        Some(codeterm_plugin::media_delivery::MediaDeliveryDescriptor {
            types: vec![codeterm_plugin::media_delivery::MediaType::Binary],
        })
    }
    async fn run_media_file(
        &self,
        id: &str,
        context: &codeterm_plugin::media_delivery::RunMediaLookupContext,
    ) -> Result<codeterm_plugin::media_delivery::MediaFile, ViewError> {
        assert_eq!(id, "artifact");
        assert!(context.valid_for(&self.0));
        assert!(!codeterm_decision::physical::effects_allowed());
        Ok(codeterm_plugin::media_delivery::MediaFile {
            path: "output.bin".into(),
            content_type: codeterm_plugin::media_delivery::MediaType::Binary,
            file_name: Some("output.bin".into()),
        })
    }
    async fn call(&self, _: &str, _: Value, _: &ViewCallContext) -> Result<Value, ViewError> {
        panic!("no studio view")
    }
}
struct Artifact(std::path::PathBuf);
impl Drop for Artifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn descriptors(id: &str) -> Vec<StepDescriptor> {
    parse_descriptors(&json!([
        {"contractVersion":1,"revision":7,"id":format!("{id}.produce"),"title":"Produce","description":"Generic fixture","params":{"type":"object","properties":{}},"inputs":{},"outputs":{"json":{"kind":"json","minItems":1,"maxItems":1,"schema":{"type":"object","properties":{"n":{"type":"integer","minimum":0,"maximum":9}},"required":["n"]}},"text":{"kind":"text","minItems":1,"maxItems":1},"file":{"kind":"file","minItems":1,"maxItems":1}},"outcomes":["done","failed"],"lifecycle":{"reconcile":false,"cancel":false},"effects":{"sideEffects":false}},
        {"contractVersion":1,"revision":7,"id":format!("{id}.consume"),"title":"Consume","description":"Generic fixture","params":{"type":"object","properties":{}},"inputs":{"file":{"kind":"file","minItems":1,"maxItems":1}},"outputs":{},"outcomes":["done","failed"],"lifecycle":{"reconcile":false,"cancel":false},"effects":{"sideEffects":false}}
    ])).unwrap()
}

#[test]
fn plugin_steps_file_and_text_ports_feed_next_steps() {
    let root = tempfile::tempdir().unwrap();
    let id = format!("fixture-{}", uuid::Uuid::new_v4().simple());
    let artifact = Artifact(codeterm_service_core::paths::data_dir().join(&id));
    std::fs::create_dir(&artifact.0).unwrap();
    std::fs::write(artifact.0.join("output.bin"), BYTES).unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let service = ServiceFixture::new(mux);
    assert!(service
        .service
        .register_host_extension(Arc::new(plugin::PluginStepStoreRoot(
            root.path().join("plugin-state")
        )))
        .is_ok());
    let adapter = Arc::new(Adapter {
        service: Arc::downgrade(&service.service),
        plugin: id.clone(),
        submits: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
    });
    let manifest = codeterm_plugin::js_runtime::manifest::PluginManifest::from_json_str(
        &json!({"id":id,"displayName":"Fixture","version":"0.1.0","capabilities":{
            "graphNodes":{"steps":descriptors(&id)},
            "mediaDelivery":{"types":["application/octet-stream"]}
        }})
        .to_string(),
    )
    .unwrap();
    service
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .register_steps(
            &id,
            &manifest.capabilities.graph_nodes.steps,
            adapter.clone(),
        )
        .unwrap();
    let nodes = service.service.blueprint_pipeline_nodes();
    for descriptor in &manifest.capabilities.graph_nodes.steps {
        let entry = nodes
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["kind"] == descriptor.id)
            .unwrap();
        assert_eq!(entry["role"], "step");
        assert_eq!(entry["plugin"], id);
        assert_eq!(
            entry["descriptor"],
            serde_json::to_value(descriptor).unwrap()
        );
    }
    service
        .service
        .ext::<ViewRegistryHandle>()
        .register(&id, Arc::new(MediaProvider(id.clone())));
    let mut definition: Value = serde_yaml::from_str(
        &include_str!("../../../../../tests/fixtures/blueprint-plugin-nodes.yaml")
            .replace("fixture.", &format!("{id}.")),
    )
    .unwrap();
    #[cfg(windows)]
    let command = "[IO.File]::WriteAllText('stdin.txt',[Console]::In.ReadToEnd())";
    #[cfg(unix)]
    let command = "cat > stdin.txt";
    definition["steps"][2]["run"]["lit"] = json!(command);
    let document =
        codeterm_blueprint_types::blueprint_document::Document::from_definition(&definition)
            .unwrap();
    let spec = serde_json::from_value(document.lower().unwrap()["spec"].clone()).unwrap();
    let caller = crate::service_adapter::SentinelCaller {
        tab: creator.clone(),
        team: None,
        workspace: workspace.clone(),
        cwd: root.path().to_string_lossy().into_owned(),
        nonce: None,
    };
    let preview = service
        .service
        .blueprint_pipeline_validate(&caller, spec)
        .unwrap();
    assert!(
        preview.checks.iter().all(|check| check.passed),
        "{:?}",
        preview.checks
    );
    for step in &preview.normalized_spec.steps[..2] {
        assert!(matches!(step.kind, StepKind::Plugin { revision: 7, .. }));
    }
    codeterm_storage::test_support::seed_empty_root(&root.path().join("storage"));
    let storage =
        codeterm_storage::Storage::open(&root.path().join("storage"), Default::default()).unwrap();
    let store = Store::with_storage(&storage, codeterm_cron::clock::MockClock::starting_at(1000));
    let job = store
        .prepare_pipeline(
            preview.normalized_spec,
            crate::store::Owner {
                creator_tab_id: creator,
                workspace_id: workspace,
                team_id: None,
                cwd: root.path().to_string_lossy().into_owned(),
            },
            false,
        )
        .unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let lease = Lease {
        job,
        cancelled: Default::default(),
        event: None,
    };
    let completion = run_pipeline(
        &handle,
        &service.service,
        &lease,
        "ports-run",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(completion.state, JournalState::Done, "{completion:?}");
    assert_eq!(adapter.submits.load(Ordering::SeqCst), 2);
    assert_eq!(adapter.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(root.path().join("stdin.txt")).unwrap(),
        TEXT.as_bytes()
    );
    let record = plugin::ledger(&service.service)
        .unwrap()
        .get(&plugin::operation("ports-run", "a"))
        .unwrap()
        .unwrap();
    let mut ctx = RunCtx::new(&handle, &service.service, &lease, "ports-run");
    ctx.outputs.insert(
        "a".into(),
        ExecutionEnvelope {
            media: "json".into(),
            schema_id: None,
            value: serde_json::to_value(record.outputs).unwrap(),
            text: String::new(),
        },
    );
    assert_eq!(
        ctx.resolve_binding("a.out.json").unwrap().value,
        json!({"n":7})
    );
    let path = ctx.resolve_binding("a.out.file").unwrap();
    let path = Path::new(path.value.as_str().unwrap());
    assert!(path.starts_with(std::fs::canonicalize(&artifact.0).unwrap()));
    assert_eq!(std::fs::read(path).unwrap(), BYTES);
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
}

struct Lifecycle {
    submits: AtomicUsize,
    panic_submit: bool,
}
impl BlueprintStep for Lifecycle {
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.panic_submit,
            "simulate interruption after the submission boundary"
        );
        Ok(JobUpdate {
            job: "existing".into(),
            state: OperationState::Running,
            outcome: None,
            outputs: [("text".into(), vec![PortValue::Text { text: TEXT.into() }])].into(),
        })
    }
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        assert_eq!(call.job, Some("existing"));
        Ok(JobUpdate::unknown("existing".into()))
    }
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        assert_eq!(call.job, Some("existing"));
        Ok(JobUpdate {
            job: "existing".into(),
            state: OperationState::Canceled,
            outcome: None,
            outputs: PortValues::new(),
        })
    }
}
fn lifecycle_request() -> (StepDescriptor, StepRequest) {
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"fixture.echo","title":"Echo","description":"Generic lifecycle","params":{"type":"object","properties":{}},"inputs":{},"outputs":{"text":{"kind":"text","minItems":0,"maxItems":1}},"outcomes":["done"],"lifecycle":{"reconcile":true,"cancel":true},"effects":{"sideEffects":false}}])).unwrap().remove(0);
    let request = StepRequest {
        context: StepContext {
            scope: RunScope {
                workspace: "workspace".into(),
                team: None,
                run: "run".into(),
            },
            owner: "owner".into(),
            node: "echo".into(),
            operation: "operation".into(),
            attempt: "attempt".into(),
            node_label: "Echo".into(),
            input_lineage: vec![],
        },
        node_type: descriptor.id.clone(),
        revision: 1,
        params: json!({}),
        inputs: PortValues::new(),
    };
    (descriptor, request)
}
#[test]
fn plugin_steps_uncertain_submit_is_never_replayed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("operations.sqlite");
    let (descriptor, request) = lifecycle_request();
    let adapter = Arc::new(Lifecycle {
        submits: AtomicUsize::new(0),
        panic_submit: true,
    });
    {
        let ledger = OperationLedger::open(&path).unwrap();
        let runtime = StepRuntime {
            ledger: &ledger,
            descriptor: &descriptor,
            adapter: adapter.clone(),
            media: None,
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.drive(
                "fixture",
                &request,
                Instant::now() + Duration::from_secs(2),
                Default::default()
            )))
            .is_err()
        );
        assert_eq!(
            ledger.get("operation").unwrap().unwrap().state,
            OperationState::Submitting
        );
    }
    let ledger = OperationLedger::open(&path).unwrap();
    ledger.recover_inflight().unwrap();
    assert_eq!(
        ledger.get("operation").unwrap().unwrap().state,
        OperationState::Unknown
    );
    let runtime = StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor,
        adapter: adapter.clone(),
        media: None,
    };
    assert_eq!(
        runtime
            .drive(
                "fixture",
                &request,
                Instant::now() + Duration::from_secs(2),
                Default::default()
            )
            .unwrap()
            .state,
        OperationState::Unknown
    );
    assert_eq!(adapter.submits.load(Ordering::SeqCst), 1);
}
#[test]
fn plugin_steps_cancel_records_canceled_and_keeps_outputs() {
    let root = tempfile::tempdir().unwrap();
    let ledger = OperationLedger::open(&root.path().join("operations.sqlite")).unwrap();
    let (descriptor, request) = lifecycle_request();
    let adapter = Arc::new(Lifecycle {
        submits: AtomicUsize::new(0),
        panic_submit: false,
    });
    let runtime = StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor,
        adapter: adapter.clone(),
        media: None,
    };
    let running = runtime
        .drive(
            "fixture",
            &request,
            Instant::now() + Duration::from_secs(2),
            Default::default(),
        )
        .unwrap();
    let canceled = runtime
        .cancel(&request, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(canceled.state, OperationState::Canceled);
    assert_eq!(canceled.outputs, running.outputs);
    assert_eq!(
        runtime
            .drive(
                "fixture",
                &request,
                Instant::now() + Duration::from_secs(2),
                Default::default()
            )
            .unwrap()
            .state,
        OperationState::Canceled
    );
    assert_eq!(adapter.submits.load(Ordering::SeqCst), 1);
}
#[test]
fn plugin_steps_missing_node_holds_plugin_step_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let (service, storage, handle, lease) = super::runtime_tests::fixture(
        json!({"schema_version":2,"kind":"pipeline","name":"missing plugin","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"fixture"}}},"steps":[{"id":"missing","uses":"fixture.echo"}],"result":{"to":"notify"}}),
        root.path(),
    );
    let mut ctx = RunCtx::new(&handle, &service.service, &lease, "missing-run");
    let mut step = lease.job.pipeline().unwrap().steps[0].clone();
    if let StepKind::Plugin { revision, .. } = &mut step.kind {
        *revision = 1;
    }
    let held = plugin::run(
        &ctx,
        &step,
        &ExecutionEnvelope::from_text("fixture"),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(held.state, JournalState::Held);
    assert_eq!(
        held.reason,
        Some(JournalReason::Other("plugin_step_unavailable".into()))
    );
    let (mut descriptor, _) = lifecycle_request();
    descriptor.revision = 2;
    let adapter = Arc::new(Lifecycle {
        submits: AtomicUsize::new(0),
        panic_submit: false,
    });
    service
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .register_steps("fixture", &[descriptor], adapter.clone())
        .unwrap();
    ctx.deadline = Instant::now() + Duration::from_secs(1);
    let held = plugin::run(
        &ctx,
        &step,
        &ExecutionEnvelope::from_text("fixture"),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        held.reason,
        Some(JournalReason::Other("plugin_step_unavailable".into()))
    );
    assert_eq!(adapter.submits.load(Ordering::SeqCst), 0);
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
}
