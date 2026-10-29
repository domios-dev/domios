use super::*;
use crate::store::Store;
use codeterm_plugin::{
    blueprint_step::*,
    graph_nodes::GraphNodeRegistry,
    view_host::{ViewCallContext, ViewError, ViewProvider, ViewRegistryHandle},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Generator {
    submissions: AtomicUsize,
}

#[test]
fn plugin_steps_optional_media_ports_are_empty_unless_explicitly_bound() {
    let registry = GraphNodeRegistry::default();
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"fixture.optional","title":"Optional","description":"Fixture",
        "params":{"type":"object","properties":{}},"inputs":{"references":{"kind":"image","minItems":0,"maxItems":4},"mask":{"kind":"image","minItems":0,"maxItems":1}},"outputs":{},"outcomes":["done","failed"],"effects":{"sideEffects":false,"potentiallyPaid":false},"lifecycle":{"reconcile":false,"cancel":false}}])).unwrap();
    registry
        .register_steps(
            "fixture",
            &descriptor,
            Arc::new(Generator {
                submissions: AtomicUsize::new(0),
            }),
        )
        .unwrap();
    let registered = registry.step("fixture.optional").unwrap();
    assert!(plugin::inputs(
        &registered,
        &ExecutionEnvelope::from_text("previous prompt"),
        false
    )
    .unwrap()
    .is_empty());
    let values: PortValues = [(
        "references".into(),
        vec![PortValue::Media {
            handle: "a".repeat(64),
            media: PortKind::Image,
        }],
    )]
    .into();
    let envelope = ExecutionEnvelope {
        media: "json".into(),
        schema_id: None,
        value: serde_json::to_value(&values).unwrap(),
        text: String::new(),
    };
    assert!(plugin::inputs(&registered, &envelope, false)
        .unwrap()
        .is_empty());
    assert_eq!(
        plugin::inputs(&registered, &envelope, true).unwrap(),
        values
    );
}
impl BlueprintStep for Generator {
    fn estimate(&self, call: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        Ok(Some(Estimate {
            amount: if call.request.context.node == "expensive" {
                5
            } else {
                1
            },
            unit: "micro_credits".into(),
            valid_until_ms: 100_000,
        }))
    }
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        self.submissions.fetch_add(1, Ordering::SeqCst);
        assert!(call.request.context.run_label.contains(" run #1"));
        assert_eq!(call.request.context.run_view.len(), 64);
        let failed = call.request.context.node == "failed_take";
        Ok(JobUpdate {
            job: format!("job-{}", call.request.context.node),
            state: if failed {
                OperationState::Failed
            } else {
                OperationState::Succeeded
            },
            outcome: Some(if failed { "failed" } else { "done" }.into()),
            outputs: if failed {
                PortValues::new()
            } else {
                [(
                    "image".into(),
                    vec![PortValue::MediaSource {
                        id: "image-1".into(),
                        media: PortKind::Image,
                    }],
                )]
                .into()
            },
        })
    }
    fn reconcile(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("terminal fixture must not reconcile");
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("unsubmitted cancel must stay local");
    }
}

struct MediaProvider(
    String,
    Option<std::sync::Weak<codeterm_service_core::service::ServiceContext>>,
);
#[codeterm_plugin::async_trait]
impl ViewProvider for MediaProvider {
    fn id(&self) -> &str {
        &self.0
    }
    fn media_delivery(&self) -> Option<codeterm_plugin::media_delivery::MediaDeliveryDescriptor> {
        Some(codeterm_plugin::media_delivery::MediaDeliveryDescriptor {
            types: vec![codeterm_plugin::media_delivery::MediaType::Png],
        })
    }
    async fn run_media_file(
        &self,
        id: &str,
        context: &codeterm_plugin::media_delivery::RunMediaLookupContext,
    ) -> Result<codeterm_plugin::media_delivery::MediaFile, ViewError> {
        assert!(!codeterm_decision::physical::effects_allowed());
        assert!(!codeterm_plugin::view_host::has_view_call_context());
        assert!(context.valid_for(&self.0));
        assert_eq!(id, "image-1");
        if context.receiver_node == "review" {
            if let Some(service) = self.1.as_ref().and_then(std::sync::Weak::upgrade) {
                service
                    .mux
                    .write()
                    .get_tab_mut(&context.owner)
                    .unwrap()
                    .team_id = Some("changed-owner-team".into());
            }
        }
        Ok(codeterm_plugin::media_delivery::MediaFile {
            path: "output.png".into(),
            content_type: codeterm_plugin::media_delivery::MediaType::Png,
            file_name: Some("Generated.png".into()),
        })
    }
    async fn call(&self, _: &str, _: Value, _: &ViewCallContext) -> Result<Value, ViewError> {
        Err(ViewError::Call("not a frame fixture".into()))
    }
}

struct Artifact(std::path::PathBuf);
impl Drop for Artifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn plugin_steps_fixture_pipeline_delivers_real_image_to_agent_and_holds_after_three_jobs_one_failure(
) {
    fixture_pipeline(false, false);
}

#[test]
fn plugin_steps_owner_quote_refresh_rejects_stale_approval_and_resumes_exact_held_run_once() {
    fixture_pipeline(true, false);
}

#[test]
fn plugin_steps_media_agent_handoff_refuses_owner_scope_changed_during_resolution() {
    fixture_pipeline(false, true);
}

fn fixture_pipeline(approve: bool, scope_lost: bool) {
    let root = tempfile::tempdir().unwrap();
    let producer = format!("fixture-{}", uuid::Uuid::new_v4().simple());
    let artifact_root = codeterm_service_core::paths::data_dir().join(&producer);
    std::fs::create_dir(&artifact_root).unwrap();
    let artifact = Artifact(artifact_root);
    let image = include_bytes!("../../../../icons/32x32.png");
    std::fs::write(artifact.0.join("output.png"), image).unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .register_host_extension(Arc::new(plugin::PluginStepStoreRoot(
            root.path().join("plugin-state")
        )))
        .is_ok());
    let generator = Arc::new(Generator {
        submissions: AtomicUsize::new(0),
    });
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":format!("{producer}.generate"),"title":"Generate","description":"No paid calls",
        "params":{"type":"object","properties":{"prompt":{"type":"string","maxLength":128}},"required":["prompt"],"additionalProperties":false},
        "inputs":{"references":{"kind":"image","minItems":0,"maxItems":4},"mask":{"kind":"image","minItems":0,"maxItems":1}},"outputs":{"image":{"kind":"image","minItems":1,"maxItems":1}},"outcomes":["done","failed"],"effects":{"sideEffects":true,"potentiallyPaid":true},"lifecycle":{"reconcile":true,"cancel":false}}])).unwrap();
    fixture
        .service
        .ext::<Arc<GraphNodeRegistry>>()
        .register_steps(&producer, &descriptor, generator.clone())
        .unwrap();
    fixture.service.ext::<ViewRegistryHandle>().register(
        &producer,
        Arc::new(MediaProvider(
            producer.clone(),
            scope_lost.then(|| {
                fixture
                    .service
                    .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
                    .clone()
            }),
        )),
    );
    let manifest_json = r#"{"id":"fixture-review","displayName":"Fixture review","version":"0.1.0","capabilities":{"chatBackend":{"agentTasks":true}}}"#;
    let manifest = serde_json::from_str(manifest_json).unwrap();
    let vm = Arc::new(codeterm_plugin::js_runtime::PluginVm::from_source_with("fixture-review", r#"module.exports.default={calls:0,reviewCount(){return this.calls;},agentTaskMedia(){return ['image'];},runAgentTask(r){this.calls++;if(r.media.length!==1||r.media[0].bytes.length<32||r.media[0].bytes[0]!==137||r.media[0].bytes[1]!==80||r.media[0].lineage.port!=='image')throw Error('real image input missing');return {status:'done',output:{verdict:'pass',notes:'image bytes received'}};},abortAgentTask(){}};"#.into(), Some(manifest_json.into())).unwrap());
    fixture.service.chatbackend_registry.register_and_wait(
        "fixture-review",
        Arc::new(codeterm_plugin::js_runtime::JsChatBackend::from_manifest(
            vm.clone(),
            &manifest,
        )),
    );
    let yaml = include_str!("../../../../../tests/fixtures/blueprint-plugin-media-review.yaml")
        .replace("fixture.generate", &format!("{producer}.generate"));
    let definition: Value = serde_yaml::from_str(&yaml).unwrap();
    codeterm_storage::test_support::seed_empty_root(&root.path().join("storage"));
    let storage =
        codeterm_storage::Storage::open(&root.path().join("storage"), Default::default()).unwrap();
    let store = Store::with_storage(&storage, codeterm_cron::clock::MockClock::starting_at(1000));
    let job = store
        .prepare_document(
            definition,
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
    assert!(fixture
        .service
        .register_host_extension(Arc::new(handle.clone()))
        .is_ok());
    let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let lease = Lease {
        job: job.clone(),
        cancelled: Default::default(),
        event: None,
    };
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "media-fixture-run",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    if scope_lost {
        assert_eq!(completion.state, JournalState::Failed);
        assert_eq!(
            codeterm_plugin::js_runtime::call_json::<u32>(&vm, "reviewCount", &[]).unwrap(),
            0
        );
        assert_eq!(generator.submissions.load(Ordering::SeqCst), 1);
        storage.shutdown().wait(Duration::from_secs(10)).unwrap();
        return;
    }
    assert_eq!(completion.state, JournalState::Held);
    assert_eq!(
        codeterm_plugin::js_runtime::call_json::<u32>(&vm, "reviewCount", &[]).unwrap(),
        1
    );
    let ledger = plugin::ledger(&fixture.service).unwrap();
    let scope = RunScope {
        workspace: job.workspace_id.clone(),
        team: None,
        run: "media-fixture-run".into(),
    };
    let history = ledger.history(&scope).unwrap();
    assert_eq!(history.len(), 4);
    assert_eq!(
        history.iter().filter(|record| record.job.is_some()).count(),
        3
    );
    assert_eq!(
        history
            .iter()
            .filter(|record| record.state == OperationState::Failed)
            .count(),
        1
    );
    assert_eq!(
        history.last().unwrap().waiting.as_ref().unwrap().reason,
        WaitingReason::OverRunCeiling
    );
    assert_eq!(generator.submissions.load(Ordering::SeqCst), 3);
    let operations = ledger
        .owner_operations(&PluginJobScope {
            plugin: producer.clone(),
            workspace: job.workspace_id.clone(),
            team: None,
            owner: job.creator_tab_id.clone(),
        })
        .unwrap();
    assert_eq!(operations.len(), 4);
    let public = serde_json::to_string(&operations).unwrap();
    assert!(public.contains("waiting_for_owner_ok"));
    assert!(!public.contains("\"handle\""));
    let journal = handle
        .store
        .lock()
        .typed_pipeline_log(&job.id, 100)
        .unwrap()
        .wait(Duration::from_secs(10))
        .unwrap()
        .value;
    assert!(journal.iter().any(|entry| entry.step_name == "review"
        && entry
            .outcome
            .as_ref()
            .is_some_and(|outcome| outcome.as_str() == "pass")));
    assert!(handle
        .store
        .lock()
        .get(&job.id)
        .unwrap()
        .admission
        .plugin_hold
        .is_some());
    let first_outputs = history[0].outputs.clone();
    let waiting = history.last().unwrap();
    if approve {
        let questions = crate::plugin_owner::OwnerQuestions(
            fixture
                .service
                .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
                .clone(),
        );
        let old_key = owner_decision_key(waiting);
        questions
            .answer_operation(
                &old_key,
                &waiting.request.context.operation,
                "Check price again",
            )
            .unwrap();
        assert!(questions
            .answer_operation(
                &old_key,
                &waiting.request.context.operation,
                "Approve 5 units (micro_credits) for this step"
            )
            .is_err());
        let refreshed = ledger
            .get(&waiting.request.context.operation)
            .unwrap()
            .unwrap();
        let current_key = owner_decision_key(&refreshed);
        assert_ne!(current_key, old_key);
        questions
            .answer_operation(
                &current_key,
                &waiting.request.context.operation,
                "Approve 5 units (micro_credits) for this step",
            )
            .unwrap();
        assert!(questions
            .answer_operation(
                &current_key,
                &waiting.request.context.operation,
                "Approve 5 units (micro_credits) for this step"
            )
            .is_err());
        let resumed = Lease {
            job: handle.store.lock().get(&job.id).unwrap().clone(),
            cancelled: Default::default(),
            event: None,
        };
        assert!(resumed.job.admission.continuation.is_some());
        run_pipeline(
            &handle,
            &fixture.service,
            &resumed,
            "media-fixture-run",
            &std::cell::Cell::new(false),
        )
        .unwrap();
        let settled = ledger
            .get(&waiting.request.context.operation)
            .unwrap()
            .unwrap();
        assert_eq!(settled.state, OperationState::Succeeded);
        assert_eq!(settled.authorized.as_ref().unwrap().maximum, 5);
        assert!(settled.authorized.unwrap().run_ceiling_exception);
        assert_eq!(generator.submissions.load(Ordering::SeqCst), 4);
        assert_eq!(
            ledger
                .get(&history[0].request.context.operation)
                .unwrap()
                .unwrap()
                .outputs,
            first_outputs
        );
        assert!(handle
            .store
            .lock()
            .get(&job.id)
            .unwrap()
            .admission
            .plugin_hold
            .is_none());
        storage.shutdown().wait(Duration::from_secs(10)).unwrap();
        return;
    }
    run_input_fixture(
        &fixture.service,
        &history[0],
        artifact.0.join("output.png"),
        image,
    );
    StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor[0],
        adapter: generator.clone(),
        media: Some(&plugin::media(&fixture.service).unwrap()),
    }
    .cancel(&waiting.request, Instant::now() + Duration::from_secs(2))
    .unwrap();
    assert_eq!(
        ledger
            .get(&history[0].request.context.operation)
            .unwrap()
            .unwrap()
            .outputs,
        first_outputs
    );
    assert_eq!(generator.submissions.load(Ordering::SeqCst), 3);
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
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
        panic!("input preparation must not request credentials");
    }
    fn readable(&self, _: &std::path::Path) -> bool {
        false
    }
}

struct InputConsumer {
    service: std::sync::Weak<codeterm_service_core::service::ServiceContext>,
    source: std::path::PathBuf,
    bytes: Vec<u8>,
}
impl BlueprintStep for InputConsumer {
    fn estimate(&self, _: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        panic!("free fixture must not request a price");
    }
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String> {
        use codeterm_plugin::binary_transfer::*;
        let service = self.service.upgrade().unwrap();
        let inputs = crate::plugin_run_inputs::Inputs(self.service.clone());
        let consumer = &call.request.context.input_lineage[0].plugin;
        assert_ne!(
            consumer,
            &call
                .request
                .node_type
                .split('.')
                .next()
                .unwrap()
                .to_string()
        );
        let plugin = call.request.node_type.split('.').next().unwrap();
        let context = StepInputContext {
            plugin: plugin.into(),
            request: call.request.clone(),
            submitting: true,
            deadline: call.deadline,
            cancellation: call.cancellation.clone(),
        };
        let PortValue::Media { handle, .. } = &call.request.inputs["image"][0] else {
            panic!("fixture media missing");
        };
        let control = InputPreparation {
            abort: Default::default(),
            deadline: call.deadline,
        };
        let session =
            TransferSession::new(plugin, Default::default(), Arc::new(InputAuthority)).unwrap();
        activate(&session);
        let first = inputs
            .bind(
                plugin,
                &context,
                handle,
                "https://fixture.invalid",
                &control,
            )
            .unwrap();
        let reopened = plugin::media(&service).unwrap();
        assert!(reopened
            .resolve_input(&context.request.context, plugin, handle, PortKind::Image)
            .is_ok());
        let replacement =
            TransferSession::new(plugin, Default::default(), Arc::new(InputAuthority)).unwrap();
        activate(&replacement);
        let second = inputs
            .bind(
                plugin,
                &context,
                handle,
                "https://fixture.invalid",
                &control,
            )
            .unwrap();
        assert_ne!(first.handle, second.handle);
        assert_eq!(first.sha256, second.sha256);
        assert_eq!(first.size, self.bytes.len() as u64);
        let mut wrong = context.clone();
        wrong.request.context.owner = "foreign-owner".into();
        assert!(inputs
            .bind(plugin, &wrong, handle, "https://fixture.invalid", &control)
            .is_err());
        assert!(inputs
            .bind(
                plugin,
                &context,
                "unbound-input",
                "https://fixture.invalid",
                &control
            )
            .is_err());
        std::fs::write(&self.source, b"changed bytes").unwrap();
        assert!(inputs
            .bind(
                plugin,
                &context,
                handle,
                "https://fixture.invalid",
                &control
            )
            .is_err());
        std::fs::write(&self.source, &self.bytes).unwrap();
        deactivate(plugin);
        Ok(JobUpdate {
            job: "input-prepared-no-network".into(),
            state: OperationState::Succeeded,
            outcome: Some("done".into()),
            outputs: PortValues::new(),
        })
    }
    fn reconcile(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("terminal input fixture");
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        panic!("terminal input fixture");
    }
}

fn run_input_fixture(
    service: &codeterm_service_core::service::ServiceContext,
    producer: &OperationRecord,
    source: std::path::PathBuf,
    bytes: &[u8],
) {
    let consumer = format!("input-fixture-{}", uuid::Uuid::new_v4().simple());
    let adapter = Arc::new(InputConsumer {
        service: service
            .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
            .clone(),
        source,
        bytes: bytes.to_vec(),
    });
    let descriptor = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":format!("{consumer}.consume"),"title":"Consume","description":"No network",
        "params":{"type":"object","properties":{"prompt":{"type":"string","maxLength":128}},"required":["prompt"]},
        "inputs":{"image":{"kind":"image","minItems":1,"maxItems":1}},"outputs":{},"outcomes":["done","failed"],"effects":{"sideEffects":false,"potentiallyPaid":false},"lifecycle":{"reconcile":false,"cancel":false}}])).unwrap();
    let steps = service.ext::<Arc<GraphNodeRegistry>>();
    steps
        .register_steps(&consumer, &descriptor, adapter)
        .unwrap();
    let registered = steps.step(&descriptor[0].id).unwrap();
    let ledger = plugin::ledger(service).unwrap();
    let media = plugin::media(service).unwrap();
    let mut request = producer.request.clone();
    request.node_type = descriptor[0].id.clone();
    request.context.node = "consume".into();
    request.context.operation = plugin::operation(&request.context.scope.run, "consume");
    request.step_allowance = 0;
    request.inputs = [("image".into(), producer.outputs["image"].clone())].into();
    let PortValue::Media { handle, .. } = &request.inputs["image"][0] else {
        panic!("fixture output missing");
    };
    media
        .authorize_handoff(&request.context, handle, &consumer, "consume")
        .unwrap();
    request.context.input_lineage = vec![media
        .input(&request.context, &consumer, handle, PortKind::Image)
        .unwrap()];
    let result = StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor[0],
        adapter: registered.adapter,
        media: Some(&media),
    }
    .drive(
        &consumer,
        &request,
        || 1000,
        Instant::now() + Duration::from_secs(5),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.state, OperationState::Succeeded);
    assert_eq!(result.job.as_deref(), Some("input-prepared-no-network"));
}
