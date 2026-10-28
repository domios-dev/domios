use super::*;
#[test]
fn blueprint_apply_invalid_bind_is_typed_and_positioned() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("invalid.yaml");
    std::fs::write(&file, "schema_version: 2\nkind: pipeline\nname: invalid-bind\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: first\n  run: {lit: echo ready}\n- id: second\n  run: {bind: first.out}\n").unwrap();
    let error = authoring::apply_request_error(&file.to_string_lossy(), "validation failed".into());
    assert_eq!(error.code, Some(-32602));
    let diagnostics = error.data.unwrap()["diagnostics"].clone();
    let diagnostic = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["field"] == "steps[1].run")
        .unwrap();
    assert_eq!(diagnostic["line"], 9);
    assert!(diagnostic["hint"].as_str().unwrap().contains("bind.in"));
}
fn files(root: &std::path::Path) -> std::path::PathBuf {
    std::fs::write(root.join("helper.yaml"), "schema_version: 2\nkind: pipeline\nname: helper\ninputs:\n  p: {type: integer}\nsteps:\n- id: value\n  uses: filter\n  with: {pattern: {lit: '.*'}}\n  in: {type: integer}\n  out: {type: integer}\nresult: {type: {type: integer}, from: value.out}\n").unwrap();
    let path = root.join("caller.yaml");
    std::fs::write(&path, "schema_version: 2\nkind: pipeline\nname: caller\ntrigger: {type: schedule, every: {lit: 20s}}\nimports: {helper: helper.yaml}\nsteps:\n- id: invoke\n  call: {ref: helper, with: {p: {lit: 7}}}\n  out: {type: integer}\nresult: {to: notify}\n").unwrap();
    path
}
#[test]
fn blueprint_v2_call_review_import_sends_local_bytes_for_host_admission() {
    let root = tempfile::tempdir().unwrap();
    let path = files(root.path());
    let (method, params) = request(
        BlueprintAction::Import {
            file: path,
            scope: Some("general".into()),
            set: vec![],
            stopped: true,
            start: false,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(method, "blueprint.import");
    assert_eq!(params["scope"], "general");
    assert_eq!(params["local_callees"].as_array().unwrap().len(), 1);
    assert_eq!(params["local_callees"][0]["document"]["name"], "helper");
    assert!(params["recipe"]["definition"]["imports"]["helper"]
        .as_str()
        .unwrap()
        .starts_with("local:"));
}
#[test]
fn blueprint_v2_call_review_update_sends_local_bytes_for_host_admission() {
    let root = tempfile::tempdir().unwrap();
    let path = files(root.path());
    let (method, params) = request(
        BlueprintAction::Update {
            id: "job".into(),
            expect_version: 1,
            from_file: path,
            note: None,
            cause: None,
            dry_run: true,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(method, "blueprint.update");
    assert_eq!(params["dry_run"], true);
    assert_eq!(params["local_callees"].as_array().unwrap().len(), 1);
}
#[test]
fn blueprint_v2_call_review_exported_pinned_caller_updates_without_reopening_imports() {
    let root = tempfile::tempdir().unwrap();
    let path = files(root.path());
    let source = std::fs::read_to_string(&path).unwrap();
    let document = codeterm_blueprint_types::blueprint_document::calls::resolve_local(
        &path.to_string_lossy(),
        &source,
    )
    .unwrap()
    .document();
    let pinned = serde_json::to_value(&document).unwrap();
    std::fs::write(&path, document.yaml().unwrap()).unwrap();
    std::fs::remove_file(root.path().join("helper.yaml")).unwrap();
    let (_, params) = request(
        BlueprintAction::Update {
            id: "job".into(),
            expect_version: 1,
            from_file: path,
            note: None,
            cause: None,
            dry_run: true,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(params["definition"]["dependencies"], pinned["dependencies"]);
    assert_eq!(
        params["definition"]["steps"][0]["call"]["pin"],
        pinned["steps"][0]["call"]["pin"]
    );
}
#[test]
fn blueprint_v2_call_review_check_nested_callable_does_not_bind_signature() {
    let root = tempfile::tempdir().unwrap();
    let path = files(root.path());
    let source = std::fs::read_to_string(&path).unwrap();
    let mut document = codeterm_blueprint_types::blueprint_document::parse("caller", &source)
        .unwrap()
        .document;
    document.trigger = None;
    document.result.to = None;
    document.result.port = Some(serde_json::from_value(json!({"type":"integer"})).unwrap());
    document.result.from = Some("invoke.out".into());
    document.inputs.insert(
        "p".into(),
        serde_json::from_value(json!({"type":"integer"})).unwrap(),
    );
    document.steps[0].call.as_mut().unwrap().arguments.insert(
        "p".into(),
        codeterm_blueprint_types::blueprint_document::ValueForm::input("p"),
    );
    std::fs::write(&path, document.yaml().unwrap()).unwrap();
    let (method, params) = request(
        BlueprintAction::Check {
            source: path.to_string_lossy().into(),
            set: vec![],
            explain: false,
            fix: false,
            write: false,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(method, "blueprint.apply");
    assert_eq!(params["dry_run"], true);
    assert_eq!(params["definition"]["inputs"]["p"]["type"], "integer");
    assert_eq!(
        params["definition"]["steps"][0]["call"]["with"]["p"],
        json!({"input":"p"})
    );
}
