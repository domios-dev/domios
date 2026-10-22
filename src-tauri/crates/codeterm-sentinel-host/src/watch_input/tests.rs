use super::*;
use crate::{BlueprintApplyExt, SentinelExt};
use codeterm_blueprint_types::sentinel_input::DataRef;
use codeterm_plugin::converter::{
    Conversion, ConvertError, ConvertOutput, ConvertRequest, Converter,
};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;

struct Transcript;
impl Converter for Transcript {
    fn conversions(&self) -> &[Conversion] {
        static EDGES: [Conversion; 1] = [Conversion {
            from: MediaKind::Audio,
            to: MediaKind::Text,
        }];
        &EDGES
    }
    fn convert(&self, request: &ConvertRequest) -> Result<ConvertOutput, ConvertError> {
        assert_eq!(
            std::fs::read(&request.input_path).unwrap(),
            b"opaque audio fixture"
        );
        assert_eq!(request.from, MediaKind::Audio);
        assert_eq!(request.to, MediaKind::Text);
        Ok(ConvertOutput::Text("ready".into()))
    }
}

#[test]
fn watch_audio_input_uses_registered_capability_and_cleans_files() {
    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("voice.wav");
    std::fs::write(&audio, b"opaque audio fixture").unwrap();
    let spec = InputSpec {
        kind: MediaKind::Audio,
        data: DataRef::File(audio),
        convert_to: Some(MediaKind::Text),
        convert_timeout_ms: Some(120000),
    };
    let registry = ConverterRegistry::new();
    assert!(validate_spec(&registry, &spec)
        .unwrap_err()
        .starts_with("input_runtime_unavailable:"));
    registry.register("opaque-converter-id", Arc::new(Transcript));
    let output = convert_output(
        &registry,
        root.path(),
        "watch-audio",
        &spec,
        Instant::now() + std::time::Duration::from_secs(120),
        &probe::cancellation::Cancellation::default(),
    )
    .unwrap();
    assert!(output.success);
    assert_eq!(output.text, "ready");
    assert_eq!(output.bytes, b"ready");
    let runs =
        codeterm_blueprint_types::sentinel_input::run_dir(root.path(), "watch-audio", "unused")
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
    assert!(std::fs::read_dir(runs).unwrap().next().is_none());
}

#[test]
fn watch_apply_refuses_missing_runtime_then_converts_and_records_observation() {
    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("voice.wav");
    std::fs::write(&audio, b"opaque audio fixture").unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.resolve_or_mint_team_for_parent(&tab).unwrap();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1000);
    let handle = crate::scheduler::fixture_handle_without_app(crate::store::Store::new(
        root.path().join("blueprints.json"),
        clock,
    ));
    crate::manage_fixture_ports(service, &handle);
    let caller = service.sentinel_caller(&tab, None).unwrap();
    let definition = json!({"schema_version":2,"kind":"watch","name":"Audio watch","note":"Observe transcript","steps":[{"id":"source","uses":"input","with":{"kind":{"lit":"audio"},"file":{"lit":audio},"convert_to":{"lit":"text"},"convert_timeout_ms":{"lit":120000}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ready"],"matches":{"ready":{"lit":"^ready$"}},"routes":{"ready":"@stop"}}]});
    for dry_run in [true, false] {
        let error = service
            .blueprint_apply(&caller, definition.clone(), dry_run)
            .unwrap_err();
        assert!(
            error.to_string().contains("input_runtime_unavailable"),
            "{error}"
        );
        assert!(handle.store.lock().list().is_empty());
    }
    service
        .ext::<Arc<ConverterRegistry>>()
        .register("opaque-converter-id", Arc::new(Transcript));
    let applied = service.blueprint_apply(&caller, definition, false).unwrap();
    let id = applied["id"].as_str().unwrap();
    let pending = service.sentinel_show(&caller, id).unwrap();
    assert_eq!(serde_json::to_value(&pending).unwrap()["next_run"], 1000);
    let mut due = crate::storage::commit(&handle.store, |store| store.stage_due()).unwrap();
    assert_eq!(due.len(), 1);
    let runtime = Arc::new(service.clone());
    crate::scheduler::tick(&handle, &runtime, &due.remove(0));
    let job = service.sentinel_show(&caller, id).unwrap();
    assert_eq!(job.progress.probe_state.as_deref(), Some("ready"));
    assert_eq!(job.progress.last_probe_at, Some(1000));
    assert_ne!(
        job.last_reason.as_deref(),
        Some("input_runtime_unavailable")
    );
    let wire = serde_json::to_value(&job).unwrap();
    assert!(wire.get("next_run").is_none());
    let restored: crate::store::Sentinel = serde_json::from_value(wire).unwrap();
    assert_eq!(
        restored.next_run,
        codeterm_blueprint_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS
    );
    let message = handle
        .store
        .lock()
        .typed_pipeline_log(id, 20)
        .err()
        .unwrap();
    assert!(message.contains(&format!("domios blueprint show {id}")));
}
