use super::*;
use crate::{
    pipeline::{execute_step, RunCtx},
    store::{Lease, Owner, Store},
};
use codeterm_blueprint_types::{
    blueprint_execution::ExecutionEnvelope, blueprint_journal::JournalState,
};
use codeterm_cron::clock::MockClock;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use codeterm_storage::{Storage, StorageConfig};
use std::time::Duration;

fn qualify(id: &str, arguments: Value, expected: Value) {
    let document = codeterm_mem::blueprint_library::catalog::document(id).unwrap();
    let assignments = document
        .inputs
        .keys()
        .map(|key| {
            let value = arguments[key]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| arguments[key].to_string());
            format!("{key}={value}")
        })
        .collect::<Vec<_>>();
    let mut definition = document.bind(&assignments).unwrap();
    definition["trigger"] = json!({"type":"schedule","every":{"lit":"1h"}});
    definition["result"] = json!({"to":"notify"});
    let root = tempfile::tempdir().unwrap();
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("test-agent".into());
    let fixture = ServiceFixture::new(mux);
    let storage = Storage::open(&root.path().join("storage"), StorageConfig::default()).unwrap();
    let store = Store::with_storage(&storage, MockClock::starting_at(1000));
    let job = store
        .prepare_document(
            definition,
            Owner {
                creator_tab_id: creator,
                team_id: None,
                workspace_id: workspace,
                cwd: root.path().to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let job = crate::storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let lease = Lease {
        job,
        cancelled: Default::default(),
        event: None,
    };
    let mut context = RunCtx::new(
        &handle,
        &fixture.service,
        &lease,
        "catalog-native-qualification",
    );
    let argument = if arguments.as_object().unwrap().len() == 1 {
        arguments
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone()
    } else {
        arguments
    };
    let mut output = ExecutionEnvelope {
        media: "json".into(),
        schema_id: None,
        value: argument,
        text: String::new(),
    };
    for step in &lease.job.pipeline().unwrap().steps {
        let result = execute_step(&mut context, step, output).unwrap_or_else(|reason| {
            panic!("{id}/{}: native execution refused: {reason:?}", step.name)
        });
        assert_eq!(
            result.state,
            JournalState::Done,
            "{id}: {} {:?}",
            result.summary,
            result.reason
        );
        output = result.output;
    }
    assert!(
        document
            .result
            .port
            .as_ref()
            .unwrap()
            .accepts_with_types(&output.value, &document.types),
        "{id}: {}",
        output.value
    );
    assert_eq!(output.value, expected, "{id}");
    assert_eq!(handle.probe_owner.snapshot().unconfirmed, 0);
    drop(context);
    drop(handle);
    drop(fixture);
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
}

#[test]
fn blueprint_v2_intent_catalog_native_checks_fan_in() {
    qualify(
        "catalog:checks-fan-in",
        json!({"args":{"checks":[{"name":"ci","ok":true,"detail":"all green"},{"name":"runners","ok":false,"detail":"0 online"}]}}),
        json!({"ready":false,"passed":1,"blockers":["runners: 0 online"],"summary":"1/2 checks pass; blocked by runners"}),
    );
}
#[test]
fn blueprint_v2_intent_catalog_native_command_lines() {
    let command = if cfg!(windows) {
        "Write-Output 'disk ok', '', 'mem ok'"
    } else {
        "printf 'disk ok\\n\\nmem ok\\n'"
    };
    qualify(
        "catalog:command-lines",
        json!({"command":command}),
        json!({"count":2,"lines":["disk ok","mem ok"]}),
    );
}
#[test]
fn blueprint_v2_intent_catalog_native_latency_percentiles() {
    qualify(
        "catalog:latency-percentiles",
        json!({"args":{"samples":[120,130,180,210,250,900,1400,140,160,175],"slo_ms":800}}),
        json!({"count":10,"p50":175,"p95":1400,"max":1400,"breach":true}),
    );
}
#[test]
fn blueprint_v2_intent_catalog_native_log_dedupe() {
    qualify(
        "catalog:log-dedupe",
        json!({"args":{"log":"error 12\nerror 13\nready","top":5}}),
        json!({"total":3,"unique":2,"patterns":[{"pattern":"error <n>","count":2,"sample":"error 12"},{"pattern":"ready","count":1,"sample":"ready"}]}),
    );
}
#[test]
fn blueprint_v2_intent_catalog_native_rule_classifier() {
    qualify(
        "catalog:rule-classifier",
        json!({"args":{"text":"Checkout returns HTTP 500","rules":{"outage":[r"\b5\d\d\b"]}}}),
        json!({"bucket":"outage","matched":"500"}),
    );
}
#[test]
fn blueprint_v2_intent_catalog_native_severity_grade() {
    qualify(
        "catalog:severity-grade",
        json!({"args":{"signal":{"status":200,"latency_ms":5200,"error_rate":0.02},"slow_ms":1500,"error_budget":0.05}}),
        json!({"severity":"high","reason":"latency 5200 ms over 3x 1500 ms"}),
    );
}
