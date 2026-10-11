use std::sync::Arc;

use super::super::host_api::PluginSandbox;
use super::super::vm::{call_json, PluginVm};

#[test]
fn parity_project_write_grant_is_the_native_and_js_ceiling() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let file = project.join("result.txt");
    for approved in [false, true] {
        let sandbox = Arc::new(
            PluginSandbox::restricted(Vec::new(), Vec::new()).with_project_roots(
                vec![project.clone()],
                if approved {
                    vec![project.clone()]
                } else {
                    Vec::new()
                },
                root.path().join("grants.yaml"),
            ),
        );
        let native = sandbox.can_write(file.to_str().unwrap());
        assert_eq!(native, approved);
        assert!(!sandbox.can_write(outside.path().join("result.txt").to_str().unwrap()));
        assert!(!sandbox.can_write(root.path().join("grants.yaml").to_str().unwrap()));
        let vm = PluginVm::from_source_sandboxed(
            "project-files",
            r#"module.exports.default = {
            write(path) { return host.writeFileAtomic(path, 'fixture'); },
            allowed(root) { return host.projectWriteAllowed(root); }
        };"#
            .into(),
            None,
            sandbox,
        )
        .unwrap();
        let js: bool = call_json(&vm, "write", &[serde_json::json!(file)]).unwrap();
        let js_allowed: bool = call_json(&vm, "allowed", &[serde_json::json!(project)]).unwrap();
        assert_eq!(js_allowed, approved);
        assert_eq!(js, native);
        assert_eq!(file.exists(), approved);
    }
}

#[cfg(unix)]
#[test]
fn project_files_refuse_symlink_escape_even_with_missing_parents() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    let project = root.path().canonicalize().unwrap();
    let sb = PluginSandbox::restricted(Vec::new(), vec![outside.path().canonicalize().unwrap()])
        .with_project_roots(
            vec![project.clone()],
            vec![project],
            root.path().join("grants.yaml"),
        );
    assert!(!sb.can_write(root.path().join("escape/missing/file").to_str().unwrap()));
    assert!(!sb.can_read(root.path().join("escape/missing/file").to_str().unwrap()));
}

#[test]
fn changed_profile_root_does_not_inherit_a_write_grant() {
    let old = tempfile::tempdir().unwrap();
    let new = tempfile::tempdir().unwrap();
    let sb = PluginSandbox::restricted(Vec::new(), Vec::new()).with_project_roots(
        vec![new.path().canonicalize().unwrap()],
        vec![old.path().canonicalize().unwrap()],
        old.path().join("grants.yaml"),
    );
    assert!(sb.can_read(new.path().join("file").to_str().unwrap()));
    assert!(!sb.can_write(new.path().join("file").to_str().unwrap()));
}
