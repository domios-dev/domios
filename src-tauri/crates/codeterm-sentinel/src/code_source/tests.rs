use super::*;

#[test]
fn blueprint_v2_runtime_code_source_reads_nested_relative_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    let source = "def main(value): return value\n";
    std::fs::write(root.path().join("nested/body.py"), source).unwrap();
    assert_eq!(read(root.path(), "nested/body.py").unwrap(), source);
    let mut code: codeterm_blueprint_types::blueprint_document::Code =
        serde_json::from_value(serde_json::json!({"lang":"python", "file":"nested/body.py"}))
            .unwrap();
    crate::seal_code::code(&mut code, root.path()).unwrap();
    assert_eq!(code.src.as_deref(), Some(source));
    assert!(code.file.is_none());
    assert_eq!(
        read(root.path(), "nested").unwrap_err(),
        "contract_violation"
    );
}

#[test]
fn blueprint_v2_runtime_code_source_refuses_parent_and_absolute_paths() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    let file = root.path().join("body.py");
    std::fs::write(&file, "def main(value): return value").unwrap();
    for relative in [
        "",
        "../body.py",
        "nested/../body.py",
        file.to_str().unwrap(),
    ] {
        assert_eq!(
            read(root.path(), relative).unwrap_err(),
            "contract_violation",
            "{relative}"
        );
    }
}

#[test]
fn blueprint_v2_runtime_code_source_enforces_sixteen_kib_bound() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("body.py");
    std::fs::write(&file, vec![b'x'; 16384]).unwrap();
    assert_eq!(read(root.path(), "body.py").unwrap().len(), 16384);
    std::fs::write(&file, vec![b'x'; 16385]).unwrap();
    assert_eq!(
        read(root.path(), "body.py").unwrap_err(),
        "budget_exhausted"
    );
}

#[cfg(unix)]
#[test]
fn blueprint_v2_runtime_code_source_refuses_symlinks_at_every_component() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("nested/body.py"), "opaque").unwrap();
    std::os::unix::fs::symlink(root.path().join("nested"), root.path().join("alias")).unwrap();
    std::os::unix::fs::symlink(
        root.path().join("nested/body.py"),
        root.path().join("body.py"),
    )
    .unwrap();
    for path in ["alias/body.py", "body.py"] {
        assert_eq!(read(root.path(), path).unwrap_err(), "contract_violation");
    }
}

#[cfg(windows)]
#[test]
fn blueprint_v2_runtime_code_source_refuses_windows_junctions() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("nested/body.py"), "inside").unwrap();
    std::fs::write(outside.path().join("body.py"), "outside").unwrap();
    for (name, target) in [
        ("inside-alias", root.path().join("nested")),
        ("outside-alias", outside.path().to_path_buf()),
    ] {
        let link = root.path().join(name);
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = read(root.path(), &format!("{name}/body.py"));
        std::fs::remove_dir(link).unwrap();
        assert_eq!(result.unwrap_err(), "contract_violation");
    }
}
