use super::*;

#[test]
fn blueprint_v2_runtime_review_memory_is_admitted_with_platform_warning() {
    let source = "schema_version: 2\nkind: pipeline\nname: memory-proof\ntrigger: {type: input, kind: {lit: text}, source: {lit: {text: hi}}}\nsteps:\n  - id: bounded\n    code:\n      lang: python\n      src: 'def main(value): return value'\n      limits: {mem_mb: 64}\nresult: {to: notify}\n";
    let checked = codeterm_blueprint_types::blueprint_document::check("memory.yaml", source, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let warnings = checked
        .diagnostics
        .iter()
        .filter(|d| d.message == "mem_mb not enforced on this OS")
        .collect::<Vec<_>>();
    if cfg!(target_os = "macos") {
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, "warning");
        assert_eq!(warnings[0].field, "steps[0].code.limits.mem_mb");
        assert_eq!(warnings[0].line, 10);
    } else {
        assert!(warnings.is_empty());
    }
    let root = tempfile::tempdir().unwrap();
    let mut doc = checked.definition.unwrap();
    doc["owner"] = json!("tab");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) =
        run_recorded(&handle, &fixture.service, &lease, "memory-admission");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_twenty_kib_text_encodes_once() {
    let text = "x".repeat(20 * 1024);
    let envelope = ExecutionEnvelope::from_text(&text);
    let encoded = envelope
        .encode()
        .expect("20 KiB text must fit the stdin envelope");
    assert!(encoded.len() < 21 * 1024);
    assert_eq!(
        ExecutionEnvelope::decode(&encoded).unwrap().payload_text(),
        text
    );
    assert_eq!(
        encoded
            .windows(1024)
            .filter(|bytes| bytes.iter().all(|b| *b == b'x'))
            .count(),
        20 * 1024 - 1023
    );
}

#[test]
fn blueprint_v2_runtime_review_ordinary_raise_routes_failed_with_stderr() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("failed-route");
    let src = "def main(value):\n raise ValueError('bad row')\nraise ValueError('bad row')\n";
    let mut doc = document("python", src, &sink);
    doc["steps"][1].as_object_mut().unwrap().remove("in");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "raise-proof");
    assert_eq!(
        completion.state,
        JournalState::Done,
        "ordinary exceptions must follow failed route; {diagnostic}"
    );
    assert!(sink.exists(), "{diagnostic}");
    let entries = journal(&handle, &lease.job.id);
    assert!(
        entries.iter().any(|entry| entry.step_name == "transform"
            && entry
                .reason
                .as_ref()
                .is_some_and(|r| r.as_str() == "code_failed")
            && entry
                .result_summary
                .as_ref()
                .is_some_and(|s| s.contains("bad row") && s.chars().count() <= 200)),
        "{diagnostic}"
    );
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_code_file_is_frozen_at_admission() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("frozen-output");
    let src = "def main(value):\n return {'n': value['n'] + 1}\n";
    let file = root.path().join("transform.py");
    std::fs::write(&file, src).unwrap();
    let mut doc = document("python", src, &sink);
    doc["steps"][0]["code"]
        .as_object_mut()
        .unwrap()
        .remove("src");
    doc["steps"][0]["code"]["file"] = json!("transform.py");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    std::fs::write(&file, "def main(value): return {'n': 99}\n").unwrap();
    let StepKind::Code { code } = &lease.job.pipeline().unwrap().steps[0].kind else {
        panic!("code step")
    };
    assert_eq!(code.file, None, "persisted root graphs must seal code.file");
    assert_eq!(code.src.as_deref(), Some(src));
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "freeze-proof");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    assert_eq!(
        ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap())
            .unwrap()
            .value,
        json!({"n":8})
    );
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_prompt_main_bind_produces_linen() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("bound-output");
    let mut doc = document("python", "def main(value): return 'linen'\n", &sink);
    doc["steps"][0]["id"] = json!("swatch");
    doc["steps"][0]["out"] = json!({"type":"string"});
    doc["steps"].as_array_mut().unwrap().insert(1, json!({"id":"caption","code":{"lang":"python","src":"def main(value): return value.upper()\n"},"bind":{"in":"swatch.out"},"in":{"type":"string"},"out":{"type":"string"}}));
    doc["steps"][2]["in"] = json!({"type":"string"});
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "bind-proof");
    assert_eq!(
        completion.state,
        JournalState::Done,
        "prompt-taught main(value) must run; {diagnostic}"
    );
    assert_eq!(
        ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap())
            .unwrap()
            .value,
        json!("LINEN")
    );
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_missing_main_is_positioned_check_error() {
    let source = "schema_version: 2\nkind: pipeline\nname: missing-main\ntrigger: {type: input, kind: {lit: text}, source: {lit: {text: hi}}}\nsteps:\n  - id: bad\n    code:\n      lang: python\n      src: 'print(7)'\nresult: {to: notify}\n";
    let checked = codeterm_blueprint_types::blueprint_document::check("missing.yaml", source, &[]);
    assert!(!checked.ok);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "steps[0].code.src"
            && d.line == 9
            && d.message.contains("main(value)")));
}

#[cfg(unix)]
#[test]
fn blueprint_v2_runtime_review_twenty_kib_command_reaches_next_step() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("text-output");
    let mut doc = document("python", "def main(value): return value", &sink);
    doc["steps"][0] = json!({"id":"text","run":{"lit":"printf '%20480s' x"}});
    doc["steps"][1].as_object_mut().unwrap().remove("in");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "text-proof");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    let output = ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap()).unwrap();
    assert_eq!(output.payload_text().len(), 20480);
    assert!(output.payload_text().ends_with('x'));
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_oversized_payload_has_explicit_truncation() {
    for text in [
        "x".repeat(100_000),
        "\0".repeat(20_000),
        "界".repeat(40_000),
    ] {
        let encoded = ExecutionEnvelope::from_text(&text).encode().unwrap();
        assert!(encoded.len() <= 32768);
        let envelope = ExecutionEnvelope::decode(&encoded).unwrap();
        assert_eq!(envelope.media, "json");
        assert_eq!(envelope.value["type"], "truncated");
        assert_eq!(envelope.value["truncated"], true);
        assert_eq!(envelope.value["total_bytes"], text.len());
        assert_eq!(envelope.value["limit_bytes"], 32768);
        assert!(text.starts_with(envelope.value["prefix"].as_str().unwrap()));
    }
}

#[test]
fn blueprint_v2_runtime_review_raising_main_redacts_and_bounds_stderr() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("failure-sink");
    let mut doc = document("python", "def main(value):\n raise ValueError('x' * 6000 + '\\nbad row\\napi_key=opaque-should-not-leak')\n", &sink);
    doc["steps"][1].as_object_mut().unwrap().remove("in");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "main-error");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    assert!(sink.exists(), "{diagnostic}");
    let entries = journal(&handle, &lease.job.id);
    let failure = entries
        .iter()
        .find(|entry| {
            entry.step_name == "transform"
                && entry
                    .reason
                    .as_ref()
                    .is_some_and(|r| r.as_str() == "code_failed")
        })
        .unwrap_or_else(|| panic!("{diagnostic}"));
    let summary = failure
        .result_summary
        .as_ref()
        .unwrap_or_else(|| panic!("{diagnostic}"));
    assert!(summary.contains("bad row"), "{diagnostic}");
    assert!(summary.contains("[redacted]"));
    assert!(!summary.contains("opaque-should-not-leak"));
    assert!(summary.chars().count() <= 200);
    assert!(!summary.chars().any(char::is_control));
    assert!(summary.contains("body_sha256="));
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_review_missing_main_refuses_before_launch() {
    use codeterm_blueprint_types::blueprint_document::{Code, Language, Limits};
    let owner = std::sync::Arc::new(crate::probe::owner::ProbeResourceOwner::new());
    let code = Code {
        lang: Language::Python,
        src: Some("print(7)".into()),
        file: None,
        caps: vec![],
        limits: Limits::default(),
    };
    let failure = crate::code_runtime::run_with_output(
        &code,
        b"{}",
        Duration::from_secs(3),
        Duration::from_secs(3),
        &owner,
        &Default::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(failure.reason, "contract_violation");
    assert_eq!(owner.snapshot().settled_total, 0);
}

#[test]
fn blueprint_v2_runtime_review_unsupported_limits_refuse_at_check() {
    for node in [
        json!({"id":"node","uses":"http","with":{"url":{"lit":"https://example.invalid"}},"limits":{"output_bytes":64}}),
        json!({"id":"node","uses":"agent","with":{"task":{"lit":"opaque"}},"limits":{"output_bytes":64}}),
        json!({"id":"node","run":{"lit":"echo opaque"},"limits":{"mem_mb":64}}),
    ] {
        let mut doc = document(
            "python",
            "def main(value): return value",
            Path::new("unused"),
        );
        doc["steps"] = json!([node]);
        let source = serde_yaml::to_string(&doc).unwrap();
        let checked =
            codeterm_blueprint_types::blueprint_document::check("limits.yaml", &source, &[]);
        assert!(!checked.ok);
        assert!(checked.diagnostics.iter().any(
            |d| d.field.starts_with("steps[0].limits") && d.message.starts_with("unsupported:")
        ));
    }
}

fn text_trigger_document(text: &str, input: Value, sink: &Path) -> Value {
    let mut doc = document("python", "def main(value): return value", sink);
    doc["trigger"]["source"]["lit"]["text"] = json!(text);
    doc["steps"][0]["in"] = input.clone();
    doc["steps"][0]["out"] = input.clone();
    doc["steps"][1]["in"] = input;
    doc
}

#[test]
fn blueprint_v2_runtime_text_trigger_digits_reach_a_string_node_as_a_string() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("string-output");
    let doc = text_trigger_document("7", json!({"type":"string"}), &sink);
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) = run_recorded(&handle, &fixture.service, &lease, "string-proof");
    assert_eq!(completion.state, JournalState::Done, "{diagnostic}");
    let delivered = ExecutionEnvelope::decode(&std::fs::read(&sink).unwrap()).unwrap();
    assert_eq!(delivered.value, json!("7"));
    drop(handle);
    drop(fixture);
    close(storage);
}

#[test]
fn blueprint_v2_runtime_text_trigger_contract_violation_names_schema_and_value() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("never-written");
    let doc = text_trigger_document("seven", json!({"type":"integer"}), &sink);
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let (completion, diagnostic) =
        run_recorded(&handle, &fixture.service, &lease, "violation-proof");
    assert_eq!(completion.state, JournalState::Failed, "{diagnostic}");
    assert_eq!(
        completion.reason,
        JournalReason::ContractViolation,
        "{diagnostic}"
    );
    let entries = journal(&handle, &lease.job.id);
    let summary = entries
        .iter()
        .find(|entry| entry.step_name == "transform" && entry.reason.is_some())
        .and_then(|entry| entry.result_summary.clone())
        .unwrap_or_default();
    assert!(summary.contains("contract_violation"), "{summary}");
    assert!(summary.contains("\"type\":\"integer\""), "{summary}");
    assert!(summary.contains("\"seven\""), "{summary}");
    assert!(!sink.exists());
    drop(handle);
    drop(fixture);
    close(storage);
}
