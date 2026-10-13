use std::sync::Arc;

use super::super::host_api::PluginSandbox;
use super::super::vm::{call_json, PluginVm};

#[test]
fn parity_private_state_root_can_be_created_without_project_write_grants() {
    let home = tempfile::tempdir().unwrap();
    let bundle = home.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest =
        super::super::PluginManifest::from_json_str(include_str!("fixtures/rotterdom/plugin.json"))
            .unwrap();
    for instance in [".codeterm-dev", ".codeterm-dev-isolated"] {
        let data = home.path().join(instance).join("rotterdom");
        let (reads, writes) = super::super::loader::confined_roots(&manifest, &bundle, &data);
        let sandbox = Arc::new(PluginSandbox::restricted(reads, writes).with_project_roots(
            Vec::new(),
            Vec::new(),
            bundle.join("grants.yaml"),
        ));
        let vm = PluginVm::from_source_sandboxed(
            "rotterdom",
            r#"module.exports.default = {
                mkdir(path) { return host.fs.makeDirs(path); },
                write(path) { return host.fs.writeFileAtomic(path, 'persisted'); },
                read(path) { return host.fs.readFile(path); }
            };"#
            .into(),
            None,
            sandbox.clone(),
        )
        .unwrap();
        assert!(!data.exists());
        assert!(!sandbox.project_write_allowed(data.to_str().unwrap()));
        let nested = data.join("new/nested");
        for path in [&data, &nested] {
            let native = sandbox.can_write(path.to_str().unwrap());
            assert!(native);
            let js: bool = call_json(&vm, "mkdir", &[serde_json::json!(path)]).unwrap();
            assert_eq!(js, native);
            assert!(path.is_dir());
        }
        let file = nested.join("history.json");
        let written: bool = call_json(&vm, "write", &[serde_json::json!(file)]).unwrap();
        assert!(written);
        let read: String = call_json(&vm, "read", &[serde_json::json!(file)]).unwrap();
        assert_eq!(read, "persisted");
        assert_eq!(std::fs::read_to_string(file).unwrap(), "persisted");
        for forbidden in [
            home.path().join(".codeterm/rotterdom"),
            home.path().join(instance).join("other-plugin"),
            home.path().join(".codeterm-dev-other/rotterdom"),
            data.join("../outside"),
            bundle.join("grants.yaml"),
        ] {
            let native = sandbox.can_write(forbidden.to_str().unwrap());
            assert!(!native);
            let js: bool = call_json(&vm, "mkdir", &[serde_json::json!(forbidden)]).unwrap();
            assert_eq!(js, native);
            assert!(!forbidden.exists());
        }
    }
}

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
    let alias_dir = tempfile::tempdir().unwrap();
    let alias = alias_dir.path().join("parent");
    std::os::unix::fs::symlink(root.path().parent().unwrap(), &alias).unwrap();
    let alias_project = alias.join(root.path().file_name().unwrap());
    assert_eq!(alias_project.canonicalize().unwrap(), project);
    for outside_is_project in [false, true] {
        let outside_root = outside.path().canonicalize().unwrap();
        let mut projects = vec![project.clone()];
        if outside_is_project {
            projects.push(outside_root.clone());
        }
        let sb = Arc::new(
            PluginSandbox::restricted(vec![outside_root.clone()], vec![outside_root])
                .with_project_roots(projects.clone(), projects, root.path().join("grants.yaml")),
        );
        let vm = PluginVm::from_source_sandboxed(
            "project-escape",
            r#"module.exports.default = {
                write(path) { return host.writeFileAtomic(path, 'fixture'); },
                read(path) { return host.readFile(path); }
            };"#
            .into(),
            None,
            sb.clone(),
        )
        .unwrap();
        for base in [root.path(), project.as_path(), alias_project.as_path()] {
            for suffix in ["escape/file", "escape/missing/file"] {
                let path = base.join(suffix);
                assert!(!sb.can_write(path.to_str().unwrap()));
                assert!(!sb.can_read(path.to_str().unwrap()));
                let written: bool = call_json(&vm, "write", &[serde_json::json!(path)]).unwrap();
                assert!(!written);
            }
            assert!(sb.can_write(base.join("missing/file").to_str().unwrap()));
            assert!(sb.can_read(base.join("missing/file").to_str().unwrap()));
        }
        let outside_file = outside.path().join("file");
        assert!(sb.can_write(outside_file.to_str().unwrap()));
        let written: bool = call_json(&vm, "write", &[serde_json::json!(outside_file)]).unwrap();
        assert!(written);
        let escaped_read: Option<String> = call_json(
            &vm,
            "read",
            &[serde_json::json!(root.path().join("escape/file"))],
        )
        .unwrap();
        assert_eq!(escaped_read, None);
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "fixture");
        assert!(!outside.path().join("missing").exists());
    }
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
