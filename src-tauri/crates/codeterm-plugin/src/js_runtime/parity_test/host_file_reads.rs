use super::*;

/// DN-2 read-preference parity: a stored transcript path whose filename
/// differs from the session id must be what BOTH the native and JS readers
/// resolve — neither may consult id-based reconstruction. The JS fixture's own
/// resolver is a reconstruction decoy: if the evidence preference were skipped
/// on the JS path, the decoy path would come back instead of the stored file.
#[test]
fn transcript_path_evidence_native_js_read_parity() {
    let tmp = tempfile::tempdir().unwrap();
    let evidence_dir = tmp.path().join("evidence");
    let _env = EnvGuard::new(&[(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        evidence_dir.to_str().unwrap(),
    )]);

    let sid = "parity-evidence-sid-4aaa-8aaa-aaaaaaaaaaaa";
    let stored = tmp.path().join("vendor-named-differently.jsonl");
    std::fs::write(&stored, "{\"cwd\":\"x\"}\n").unwrap();
    assert!(crate::session_evidence::record(
        sid,
        stored.to_str().unwrap()
    ));

    let cwd = tmp.path().to_string_lossy().to_string();

    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let native_path = native
        .session_file_path(&cwd, sid)
        .expect("native reader resolves the stored path");
    assert_eq!(native_path, stored, "native reader prefers the stored file");

    let js = load_inline_js(
        "transcript-evidence-fixture",
        r#"module.exports.default = {
            sessionFilePath(cwd, sessionId) { return cwd + "/" + sessionId + ".jsonl"; }
        };"#,
    );
    let js_path = js
        .session_file_path(&cwd, sid)
        .expect("js reader resolves the stored path");
    assert_eq!(
        js_path, native_path,
        "native and JS readers agree on the stored file"
    );
    assert_ne!(
        js_path,
        PathBuf::from(format!("{cwd}/{sid}.jsonl")),
        "the JS reconstruction decoy was not consulted"
    );
    assert_eq!(
        js.session_jsonl_path(&cwd, sid).as_deref(),
        Some(native_path.as_path()),
        "the chat-tail resolver honors the same stored path"
    );
    assert!(
        std::fs::read_to_string(&js_path).is_ok(),
        "both readers read the stored file itself"
    );

    // Absence control: no evidence ⇒ the JS resolver's own answer comes back.
    let plain_sid = "parity-no-evidence-sid";
    assert_eq!(
        js.session_file_path(&cwd, plain_sid),
        Some(PathBuf::from(format!("{cwd}/{plain_sid}.jsonl"))),
        "absence of evidence keeps today's resolution byte-for-byte"
    );
}

#[test]
fn host_read_file_refuses_past_cap() {
    const LIMIT: usize = 1024 * 1024;
    let dir = tempfile::tempdir().unwrap();
    let small_path = dir.path().join("small.txt");
    let large_path = dir.path().join("large.txt");
    std::fs::write(&small_path, b"bounded fixture").unwrap();
    std::fs::write(&large_path, vec![b'x'; LIMIT + 1]).unwrap();

    let vm = super::super::vm::PluginVm::from_source(
        "read-file-cap-fixture",
        "module.exports.default = { read(path) { return host.fs.readFile(path); } };".into(),
    )
    .unwrap();
    let small = small_path.to_string_lossy().into_owned();
    let native = super::super::host_api::read_plugin_file(&small).unwrap();
    let javascript: Option<String> =
        super::super::vm::call_json(&vm, "read", &[serde_json::json!(small)]).unwrap();
    assert!(native.is_some());
    assert_eq!(javascript, native);

    let missing = dir
        .path()
        .join("missing.txt")
        .to_string_lossy()
        .into_owned();
    assert_eq!(super::super::host_api::read_plugin_file(&missing), Ok(None));
    let javascript_missing: Option<String> =
        super::super::vm::call_json(&vm, "read", &[serde_json::json!(missing)]).unwrap();
    assert_eq!(javascript_missing, None);

    let large = large_path.to_string_lossy().into_owned();
    let native_error = super::super::host_api::read_plugin_file(&large).unwrap_err();
    let javascript_error =
        super::super::vm::call_json::<String>(&vm, "read", &[serde_json::json!(large)])
            .unwrap_err();
    assert!(native_error.contains("1048576"));
    assert!(javascript_error.to_string().contains("1048576"));
}
