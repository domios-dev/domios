use super::*;

const PIPELINE: &str = r#"schema_version: 1
name: Endpoint health
note: Alert on outages
owner: team
inputs:
  url: {type: string, default: 'https://example.com/health'}
  status: {type: integer, default: 204}
pipeline:
  trigger: {type: schedule, every: 1m}
  steps:
    - name: health
      input: text
      uses: http
      with: {url: '{{ url }}', expected_status: '{{status}}', timeout_ms: 3000}
      on: {up: stop, down: diagnose, failed: diagnose}
    - name: diagnose
      input: text
      run: echo diagnose
      on: {done: stop, failed: stop}
  result: {to: notify}
"#;

#[test]
fn yaml_spec_yaml_round_trip_preserves_nodes_routes_and_native_inputs() {
    let checked = check("health.yaml", PIPELINE, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let definition = checked.definition.unwrap();
    assert_eq!(
        definition.pointer("/spec/steps/0/kind/probe/expected_status"),
        Some(&json!("204"))
    );
    let spec: crate::blueprint_pipeline::BlueprintSpec =
        serde_json::from_value(definition["spec"].clone()).unwrap();
    assert_eq!(spec.steps[1].name, "diagnose");
    let document = Document::from_definition(&definition).unwrap();
    let raw = document.yaml().unwrap();
    assert!(raw.contains("uses: http"));
    assert!(raw.contains("run: echo diagnose"));
    assert!(!raw.contains("definition:"));
    assert_eq!(check("export.yaml", &raw, &[]).definition, Some(definition));
}

#[test]
fn multiple_errors_have_exact_source_positions_and_hints() {
    let source = PIPELINE
        .replace("every: 1m", "every: tomorrow")
        .replace("down: diagnose", "down: missing")
        .replace("uses: http", "uses: htp");
    let checked = check("bad.yaml", &source, &[]);
    assert!(!checked.ok);
    let unknown = checked
        .diagnostics
        .iter()
        .find(|d| d.field == "pipeline.steps[0].uses")
        .unwrap();
    assert_eq!((unknown.line, unknown.column), (13, 7));
    assert!(unknown.message.contains("htp"));
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "pipeline.trigger.every"));
    assert!(checked
        .diagnostics
        .iter()
        .all(|d| d.line > 0 && d.column > 0 && !d.hint.is_empty()));
}

#[test]
fn bad_routes_and_unreachable_stages_are_errors() {
    let source = PIPELINE.replace(
        "down: diagnose, failed: diagnose",
        "down: nowhere, failed: stop",
    );
    let checked = check("bad-route.yaml", &source, &[]);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.message.contains("does not exist")));
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.message == "Unreachable stage"));
}

#[test]
fn input_type_defaults_enums_and_all_missing_bindings_are_checked() {
    let source = PIPELINE.replace("default: 204", "default: nope").replace(
        "default: 'https://example.com/health'",
        "enum: ['https://example.com/health']",
    );
    let checked = check("inputs.yaml", &source, &[]);
    assert!(!checked.ok);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "inputs.status.default"));
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "inputs.url" && d.message.contains("Missing")));
    assert!(!check("inputs.yaml", PIPELINE, &["other=value".into()]).ok);
}

#[test]
fn literals_in_commands_are_opaque_and_declared_inputs_allow_spaces() {
    let source = PIPELINE.replace(
        "run: echo diagnose",
        r#"run: "docker ps --format '{{.Status}}' --filter '{{ url }}'"#,
    );
    let checked = check("literal.yaml", &source, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    assert_eq!(
        checked.definition.unwrap()["spec"]["steps"][1]["kind"]["command"],
        "docker ps --format '{{.Status}}' --filter 'https://example.com/health'"
    );
}

#[test]
fn duplicate_keys_report_the_duplicate_location_in_block_and_flow_yaml() {
    for (source, line, column) in [
        ("schema_version: 1\nname: A\nname: B\nwatch: {}\n", 3, 1),
        ("{schema_version: 1, name: A, name: B, watch: {}}", 1, 30),
    ] {
        let checked = check("duplicate.yaml", source, &[]);
        let duplicate = checked
            .diagnostics
            .iter()
            .find(|d| d.message == "Duplicate mapping key")
            .unwrap();
        assert_eq!((duplicate.line, duplicate.column), (line, column));
    }
}

#[test]
fn every_rejected_document_has_an_error_and_syntax_has_positions() {
    for source in [
        "[]",
        "null",
        "name: [",
        "schema_version: nope",
        "schema_version: 1\nname: A\nwatc: {}",
        "schema_version: 1\nname: A\nwatch: {}\npipeline: {}",
    ] {
        let checked = check("invalid.yaml", source, &[]);
        assert!(!checked.ok);
        assert!(
            checked.diagnostics.iter().any(|d| d.severity == "error"),
            "{source}"
        );
    }
}

#[test]
fn watch_validation_uses_shared_admission_and_round_trips() {
    let source="schema_version: 1\nname: Build result\nwatch:\n  probe: echo done\n  question: State?\n  buckets: [done]\n  matches: {done: done}\n  every: 20s\n  timeout: 1h\n  handlers:\n    done: [{type: notify}]\n";
    let checked = check("watch.yaml", source, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.severity == "warning" && d.field == "note"));
    let definition = checked.definition.unwrap();
    assert!(!definition["create"]["handlers"]["done"][0]["id"]
        .as_str()
        .unwrap()
        .is_empty());
    let create: crate::blueprint_watch::Create =
        serde_json::from_value(definition["create"].clone()).unwrap();
    assert!(create.admission_errors().is_empty());
    let document = Document::from_definition(&definition).unwrap();
    assert_eq!(
        check("export.yaml", &document.yaml().unwrap(), &[]).definition,
        Some(definition)
    );
    for bad in [
        source.replace("every: 20s", "every: 1s"),
        source.replace("timeout: 1h", "timeout: tomorrow"),
        source.replace("type: notify", "type: send"),
        source.replace("name: Build result", "name: Build result\nowner: team"),
    ] {
        assert!(!check("bad.yaml", &bad, &[]).ok, "{bad}");
    }
}

#[test]
fn warnings_do_not_fail_and_native_object_inputs_round_trip() {
    let source=PIPELINE.replace("note: Alert on outages\n","").replace("  status: {type: integer, default: 204}","  status: {type: integer, default: 204}\n  params: {type: object, default: {resource: demo}} ").replace("trigger: {type: schedule, every: 1m}","trigger: {type: event, kind: custom_event, params: '{{params}}'}");
    let checked = check("warnings.yaml", &source, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    assert!(checked.diagnostics.iter().any(|d| d.field == "note"));
    assert_eq!(
        checked.definition.unwrap()["spec"]["trigger"]["params"],
        json!({"resource":"demo"})
    );
}
use crate::blueprint_watch::{duration_ms, Create, Route};

#[test]
fn watch_route_and_duration_grammar_is_shared_with_admission() {
    for route in [
        "notify",
        "stop",
        "send:agent_1",
        "cron:nightly",
        "blueprint:blueprint_123",
        "escalate:creator",
        "escalate:agent:agent_1",
        "respond_prompt:permission",
    ] {
        assert!(Route::parse(route).is_ok(), "{route}")
    }
    for route in [
        "none",
        "blueprint:wrong",
        "send:",
        "send:foreign/target",
        "escalate:agent:a/b",
    ] {
        assert!(Route::parse(route).is_err(), "{route}")
    }
    assert_eq!(duration_ms("1h30m").unwrap(), 5400000);
    for duration in ["", "0s", "1s2", "tomorrow", "99999999999999999999d"] {
        assert!(duration_ms(duration).is_err(), "{duration}")
    }
}

#[test]
fn watch_all_invalid_parameters_are_reported_before_runtime_admission() {
    let mut create:Create=serde_json::from_value(json!({"name":"Watch","probe":"echo done","question":"State?","buckets":["done"],"every":"1s","timeout":"tomorrow","until":"missing","rearm_on_change":["unknown"],"handlers":{"done":[{"type":"command","parameters":{"command":"echo"}},{"type":"send","parameters":{}}]}})).unwrap();
    create.normalize_handlers();
    let errors = create.admission_errors();
    for field in [
        "every",
        "timeout",
        "until",
        "rearm_on_change",
        "handlers.done[0].parameters",
        "handlers.done[1].parameters",
    ] {
        assert!(
            errors.iter().any(|(key, _)| key == field),
            "{field}: {errors:?}"
        )
    }
}
#[test]
fn undeclared_simple_inputs_warn_but_command_templates_remain_opaque() {
    let source = PIPELINE.replace(
        "run: echo diagnose",
        r#"run: "echo '{{typo}}' '{{.Status}}' '{{range .}}'"#,
    );
    let checked = check("literal.yaml", &source, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let warnings = checked
        .diagnostics
        .iter()
        .filter(|d| d.message.starts_with("Undeclared input"))
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("typo"));
}
