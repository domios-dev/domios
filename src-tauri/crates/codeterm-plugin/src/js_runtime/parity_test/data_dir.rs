use super::super::vm::{call_json, PluginVm};

#[test]
fn data_dir_native_js_parity_with_instance_override() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("custom-instance");
    let root_text = root.to_str().unwrap();
    let _env = super::EnvGuard::new(&[("CODETERM_DATA_DIR", root_text)]);
    let native = codeterm_paths::data_dir_path();
    let vm = PluginVm::from_source(
        "data-dir-parity",
        r#"module.exports.default = { root() { return host.dataDir(); } };"#.into(),
    )
    .unwrap();
    let js: String = call_json(&vm, "root", &[]).unwrap();
    assert_eq!(std::path::PathBuf::from(js), native);
    assert_eq!(native, root);
    assert!(!root.exists(), "reading the root must not create it");
}
