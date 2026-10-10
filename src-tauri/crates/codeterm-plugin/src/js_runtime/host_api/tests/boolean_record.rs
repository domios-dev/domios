use super::*;

#[test]
fn json_boolean_record_preserves_sandbox_read_and_write_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let record = temp.path().join("record.json");
    std::fs::write(&record, "{}").unwrap();
    let source = r#"module.exports.default = {
        record(path, grant) { return JSON.parse(host.jsonBooleanRecord(path, '["trusted"]', grant)); },
        canonical(path) { return host.canonicalPath(path); }
    };"#;
    for (reads, writes, can_read, can_write) in [
        (vec![], vec![], false, false),
        (vec![temp.path().to_path_buf()], vec![], true, false),
        (
            vec![temp.path().to_path_buf()],
            vec![temp.path().to_path_buf()],
            true,
            true,
        ),
    ] {
        std::fs::write(&record, "{}").unwrap();
        let sandbox = Arc::new(PluginSandbox::restricted(reads, writes));
        let vm = PluginVm::from_source_sandboxed("record-test", source.to_string(), None, sandbox)
            .unwrap();
        let path = record.to_str().unwrap();
        let outcome: crate::provider::PreTrustOutcome =
            crate::js_runtime::vm::call_json(&vm, "record", &[json!(path), json!(true)]).unwrap();
        assert_eq!(
            matches!(outcome, crate::provider::PreTrustOutcome::Granted),
            can_write
        );
        let canonical: Option<String> =
            crate::js_runtime::vm::call_json(&vm, "canonical", &[json!(path)]).unwrap();
        assert_eq!(canonical.is_some(), can_read);
        if !can_write {
            assert_eq!(std::fs::read_to_string(&record).unwrap(), "{}");
        }
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}
