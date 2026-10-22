use super::*;

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_review_memory_is_positioned_refusal() {
    let source = "schema_version: 2\nkind: pipeline\nname: memory-proof\ntrigger: {type: input, kind: {lit: text}, source: {lit: {text: hi}}}\nsteps:\n  - id: bounded\n    code:\n      lang: python\n      src: 'def main(value): return value'\n      limits: {mem_mb: 64}\nresult: {to: notify}\n";
    let checked = codeterm_blueprint_types::blueprint_document::check("memory.yaml", source, &[]);
    assert!(
        !checked.ok,
        "an unenforced memory budget must refuse at check"
    );
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "steps[0].code.limits.mem_mb"
            && d.line == 10
            && d.message.contains("unsupported: memory budget on macOS")));
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

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_review_ordinary_raise_routes_failed_with_stderr() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("failed-route");
    let src = "def main(value):\n raise ValueError('bad row')\nraise ValueError('bad row')\n";
    let mut doc = document("python", src, &sink);
    doc["steps"][1].as_object_mut().unwrap().remove("in");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "raise-proof",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(
        completion.state,
        JournalState::Done,
        "ordinary exceptions must follow failed route"
    );
    assert!(sink.exists());
    let entries = journal(&handle, &lease.job.id);
    assert!(entries.iter().any(|entry| entry.step_name == "transform"
        && entry
            .reason
            .as_ref()
            .is_some_and(|r| r.as_str() == "code_failed")
        && entry
            .result_summary
            .as_ref()
            .is_some_and(|s| s.contains("bad row") && s.len() <= 2048)));
    drop(handle);
    drop(fixture);
    close(storage);
}

#[cfg(target_os = "macos")]
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
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "freeze-proof",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(completion.state, JournalState::Done);
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

#[cfg(target_os = "macos")]
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
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "bind-proof",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(
        completion.state,
        JournalState::Done,
        "prompt-taught main(value) must run"
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

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_review_caught_permission_error_still_denied() {
    let root = tempfile::tempdir().unwrap();
    let forbidden = root.path().join("forbidden");
    let path = serde_json::to_string(&forbidden).unwrap();
    let src = format!("import json,sys\ndef main(value): return value\ntry:\n open({path}, 'w').write('denied')\nexcept PermissionError: pass\njson.dump({{'media':'json','schema_id':None,'value':{{'n':8}},'text':''}},sys.stdout)\n");
    prove_pipeline("python", &src, Some(JournalReason::SandboxDenied));
    assert!(!forbidden.exists());
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
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "text-proof",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(completion.state, JournalState::Done);
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

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_review_raising_main_redacts_and_bounds_stderr() {
    let root = tempfile::tempdir().unwrap();
    let sink = root.path().join("failure-sink");
    let mut doc = document("python", "def main(value):\n raise ValueError('x' * 6000 + '\\nbad row\\napi_key=opaque-should-not-leak')\n", &sink);
    doc["steps"][1].as_object_mut().unwrap().remove("in");
    let (fixture, storage, handle, lease) = fixture(doc, root.path());
    let completion = run_pipeline(
        &handle,
        &fixture.service,
        &lease,
        "main-error",
        &std::cell::Cell::new(false),
    )
    .unwrap();
    assert_eq!(completion.state, JournalState::Done);
    assert!(sink.exists());
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
        .unwrap();
    let summary = failure.result_summary.as_ref().unwrap();
    assert!(summary.contains("bad row"));
    assert!(summary.contains("[redacted]"));
    assert!(!summary.contains("opaque-should-not-leak"));
    assert!(summary.len() <= 2048);
    assert!(summary.contains("body_sha256="));
    drop(handle);
    drop(fixture);
    close(storage);
}

#[cfg(target_os = "macos")]
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
    let failure = crate::code_runtime::run(
        &code,
        b"{}",
        Duration::from_secs(3),
        &owner,
        &Default::default(),
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
