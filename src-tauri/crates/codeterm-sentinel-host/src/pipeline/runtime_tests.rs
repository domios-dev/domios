use super::*;
use crate::store::{Owner, Store};
use codeterm_blueprint_types::blueprint_document::Document;
use codeterm_cron::clock::MockClock;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_storage::{Storage, StorageConfig};
use serde_json::{json, Value};

fn fixture(document: Value, root: &Path) -> (ServiceFixture, Storage, Handle, Lease) {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("test-agent".into());
    let fixture = ServiceFixture::new(mux);
    let storage = Storage::open(&root.join("storage"), StorageConfig::default()).unwrap();
    let store = Store::with_storage(&storage, MockClock::starting_at(1000));
    let job = store
        .prepare_document(
            document,
            Owner {
                creator_tab_id: creator,
                team_id: None,
                workspace_id: workspace,
                cwd: root.to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    (
        fixture,
        storage,
        handle,
        Lease {
            job,
            cancelled: Default::default(),
            event: None,
        },
    )
}

fn journal(handle: &Handle, id: &str) -> Vec<codeterm_storage::BlueprintJournalEntry> {
    let read = handle.store.lock().typed_pipeline_log(id, 100).unwrap();
    read.wait(Duration::from_secs(10)).unwrap().value
}

fn close(storage: Storage) {
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
}

fn schema() -> Value {
    json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"],"additionalProperties":false})
}

fn document(language: &str, src: &str, sink: &Path) -> Value {
    #[cfg(unix)]
    let command = format!("cat > '{}'", sink.to_string_lossy().replace('\'', "'\\''"));
    #[cfg(windows)]
    let command = format!(
        "[IO.File]::WriteAllText('{}',[Console]::In.ReadToEnd())",
        sink.to_string_lossy().replace('\'', "''")
    );
    json!({"schema_version":2,"kind":"pipeline","name":"runtime-proof","owner":"tab",
        "trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"{\"n\":7}"}}},
        "steps":[{"id":"transform","code":{"lang":language,"src":src,"limits":{"wall_ms":3000,"output_bytes":32768}},"in":schema(),"out":schema(),"routes":{"failed":"@next"}},
                 {"id":"sink","run":{"lit":command},"in":schema()}],"result":{"to":"notify"}})
}

#[cfg(target_os = "macos")]
fn prove_pipeline(language: &str, src: &str, expected: Option<JournalReason>) {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("captured-stdin.json");
    let (fixture, storage, handle, lease) = fixture(document(language, src, &sink), root.path());
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "runtime-proof",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    if let Some(reason) = expected {
        assert_eq!(completion.state, JournalState::Failed);
        assert_eq!(completion.reason, reason);
        assert!(
            !sink.exists(),
            "sink must never run after a contract or confinement failure"
        );
        let records = journal(&handle, &lease.job.id);
        assert!(records
            .iter()
            .any(|entry| entry.step_name == "transform" && entry.reason.as_ref() == Some(&reason)));
        assert!(!records.iter().any(|entry| entry.step_name == "sink"));
    } else {
        assert_eq!(
            completion.state,
            JournalState::Done,
            "{}",
            completion.reason.as_str()
        );
        let bytes = std::fs::read(&sink).unwrap();
        assert!(bytes.len() <= 32768);
        let envelope = ExecutionEnvelope::decode(&bytes).unwrap();
        assert_eq!(envelope.value, json!({"n":8}));
        let records = journal(&handle, &lease.job.id);
        assert!(records
            .iter()
            .any(|entry| entry.step_name == "transform" && entry.state == JournalState::Done));
        assert!(records
            .iter()
            .any(|entry| entry.step_name == "sink" && entry.state == JournalState::Done));
    }
    assert_eq!(handle.probe_owner.snapshot().unconfirmed, 0);
    drop(handle);
    drop(fixture);
    close(storage);
}

#[cfg(target_os = "macos")]
const PYTHON: &str =
    "import sys,json\nx=json.load(sys.stdin)\nx['value']['n']+=1\njson.dump(x,sys.stdout)\n";
#[cfg(target_os = "macos")]
const TS: &str = "import fs from 'node:fs';const x:{value:{n:number}}=JSON.parse(fs.readFileSync(0,'utf8'));x.value.n++;process.stdout.write(JSON.stringify(x));";

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_existing_pipeline_python_reaches_fixed_stdin_sink() {
    prove_pipeline("python", PYTHON, None);
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_existing_pipeline_ts_reaches_fixed_stdin_sink() {
    prove_pipeline("ts", TS, None);
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_contract_mismatch_stops_before_sink_despite_failure_route() {
    prove_pipeline(
        "python",
        &PYTHON.replace("+=1", "='wrong'"),
        Some(JournalReason::ContractViolation),
    );
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_denied_body_stops_before_sink() {
    let root = tempfile::tempdir().unwrap();
    let forbidden = root.path().join("forbidden");
    let src = format!(
        "open({},'w').write('denied')",
        serde_json::to_string(&forbidden).unwrap()
    );
    prove_pipeline("python", &src, Some(JournalReason::SandboxDenied));
    assert!(!forbidden.exists());
}

#[test]
fn blueprint_v2_runtime_port_schema_and_capability_refusal() {
    let mut doc = document("python", "pass", Path::new("opaque-sink"));
    assert!(Document::from_definition(&doc).unwrap().lower().is_ok());
    doc["steps"][0]["code"]["caps"] = json!(["domios-cli"]);
    let refusal = Document::from_definition(&doc).unwrap_err();
    assert!(refusal.contains("domios-cli capability is not approved"));
    let contract: codeterm_blueprint_types::blueprint_execution::ExecutionContract =
        serde_json::from_value(json!({"input":schema(),"output":schema()})).unwrap();
    assert!(contract
        .validate(&ExecutionEnvelope::from_text("{\"n\":7}"), false)
        .is_ok());
    for value in [json!({"n":"7"}), json!({"n":7,"extra":0}), json!({})] {
        assert_eq!(
            contract.validate(&ExecutionEnvelope::from_text(&value.to_string()), true),
            Err(JournalReason::ContractViolation)
        );
    }
}

#[test]
fn blueprint_v2_runtime_shared_context_binding_depth_and_step_budget() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("must-not-run");
    let (fixture, storage, handle, lease) = fixture(document("python", "pass", &sink), root.path());
    let mut ctx = RunCtx::new(&handle, &fixture.service, &lease, "budget-proof");
    for _ in 0..8 {
        ctx.enter_call().unwrap();
    }
    assert_eq!(ctx.enter_call(), Err(JournalReason::BudgetExhausted));
    for _ in 0..8 {
        ctx.exit_call();
    }
    ctx.outputs.insert(
        "source".into(),
        ExecutionEnvelope::from_text("{\"n\":7,\"list\":[8]}"),
    );
    assert_eq!(ctx.resolve_binding("source.out.n").unwrap().value, json!(7));
    assert_eq!(
        ctx.resolve_binding("source.out.list.0").unwrap().value,
        json!(8)
    );
    assert_eq!(
        ctx.resolve_binding("source.out.missing"),
        Err(JournalReason::ContractViolation)
    );
    let filter: Step = serde_json::from_value(json!({"name":"bounded","input":"text","kind":{"kind":"filter","pattern":".*","bucket":null}})).unwrap();
    for _ in 0..64 {
        execute_step(&mut ctx, &filter, ExecutionEnvelope::from_text("opaque")).unwrap();
    }
    assert_eq!(
        execute_step(
            &mut ctx,
            &lease.job.pipeline().unwrap().steps[1],
            ExecutionEnvelope::from_text("{\"n\":7}")
        )
        .err(),
        Some(JournalReason::BudgetExhausted)
    );
    assert!(!sink.exists());
    assert_eq!(handle.probe_owner.snapshot().settled_total, 0);
    drop(ctx);
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_input_contract_rejects_before_child_launch() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("must-not-run");
    let (fixture, storage, handle, lease) = fixture(document("python", PYTHON, &sink), root.path());
    let mut ctx = RunCtx::new(&handle, &fixture.service, &lease, "input-proof");
    let reason = execute_step(
        &mut ctx,
        &lease.job.pipeline().unwrap().steps[0],
        ExecutionEnvelope::from_text("{\"n\":\"bad\"}"),
    )
    .err();
    assert_eq!(reason, Some(JournalReason::ContractViolation));
    assert_eq!(handle.probe_owner.snapshot().settled_total, 0);
    assert!(journal(&handle, &lease.job.id)
        .iter()
        .any(|entry| entry.step_name == "transform"
            && entry.reason == Some(JournalReason::ContractViolation)));
    assert!(!sink.exists());
    drop(ctx);
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_envelope_requires_all_fields_and_preserves_text_type() {
    for value in [
        json!({"media":"json","value":7,"text":""}),
        json!({"media":"json","schema_id":null,"value":7,"text":"", "extra":1}),
        json!({"media":"text","schema_id":null,"value":"data","text":"different"}),
    ] {
        assert_eq!(
            ExecutionEnvelope::decode(value.to_string().as_bytes()),
            Err(JournalReason::ContractViolation)
        );
    }
}
